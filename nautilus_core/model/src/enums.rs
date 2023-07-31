// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

//! Defines enumerations for the trading domain model.

use std::{ffi::c_char, str::FromStr};

use nautilus_core::string::{cstr_to_string, str_to_cstr};
use pyo3::prelude::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{AsRefStr, Display, EnumString, FromRepr};

use crate::strum_serde;

pub trait FromU8 {
    fn from_u8(value: u8) -> Option<Self>
    where
        Self: Sized;
}

/// An account type provided by a trading venue or broker.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum AccountType {
    /// An account with unleveraged cash assets only.
    Cash = 1,
    /// An account which facilitates trading on margin, using account assets as collateral.
    Margin = 2,
    /// An account specific to betting markets.
    Betting = 3,
}

/// An aggregation source for derived data.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum AggregationSource {
    /// The data is externally aggregated (outside the Nautilus system boundary).
    External = 1,
    /// The data is internally aggregated (inside the Nautilus system boundary).
    Internal = 2,
}

/// The side for the aggressing order of a trade in a market.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum AggressorSide {
    /// There was no specific aggressor for the trade.
    NoAggressor = 0, // Will be replaced by `Option`
    /// The BUY order was the aggressor for the trade.
    Buyer = 1,
    /// The SELL order was the aggressor for the trade.
    Seller = 2,
}

/// A broad financial market asset class.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
#[allow(non_camel_case_types)]
pub enum AssetClass {
    /// Foreign exchange (FOREX) assets.
    FX = 1,
    /// Equity / stock assets.
    Equity = 2,
    /// Commodity assets.
    Commodity = 3,
    /// Metal commodity assets.
    Metal = 4,
    /// Energy commodity assets.
    Energy = 5,
    /// Fixed income bond assets.
    Bond = 6,
    /// Index based assets.
    Index = 7,
    /// Cryptocurrency or crypto token assets.
    Cryptocurrency = 8,
    /// Sports betting instruments.
    SportsBetting = 9,
}

/// The asset type for a financial market product.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum AssetType {
    /// A spot market asset type. The current market price of an asset that is bought or sold for immediate delivery and payment.
    Spot = 1,
    /// A swap asset type. A derivative contract through which two parties exchange the cash flows or liabilities from two different financial instruments.
    Swap = 2,
    /// A futures contract asset type. A legal agreement to buy or sell an asset at a predetermined price at a specified time in the future.
    Future = 3,
    /// A forward derivative asset type. A customized contract between two parties to buy or sell an asset at a specified price on a future date.
    Forward = 4,
    /// A contract-for-difference (CFD) asset type. A contract between an investor and a CFD broker to exchange the difference in the value of a financial product between the time the contract opens and closes.
    Cfd = 5,
    /// An options contract asset type. A type of derivative that gives the holder the right, but not the obligation, to buy or sell an underlying asset at a predetermined price before or at a certain future date.
    Option = 6,
    /// A warrant asset type. A derivative that gives the holder the right, but not the obligation, to buy or sell a security—most commonly an equity—at a certain price before expiration.
    Warrant = 7,
}

/// The aggregation method through which a bar is generated and closed.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum BarAggregation {
    /// Based on a number of ticks.
    Tick = 1,
    /// Based on the buy/sell imbalance of ticks.
    TickImbalance = 2,
    /// Based on sequential buy/sell runs of ticks.
    TickRuns = 3,
    /// Based on trading volume.
    Volume = 4,
    /// Based on the buy/sell imbalance of trading volume.
    VolumeImbalance = 5,
    /// Based on sequential runs of buy/sell trading volume.
    VolumeRuns = 6,
    /// Based on the 'notional' value of the instrument.
    Value = 7,
    /// Based on the buy/sell imbalance of trading by 'notional' value.
    ValueImbalance = 8,
    /// Based on sequential buy/sell runs of trading by 'notional' value.
    ValueRuns = 9,
    /// Based on time intervals with millisecond granularity.
    Millisecond = 10,
    /// Based on time intervals with second granularity.
    Second = 11,
    /// Based on time intervals with minute granularity.
    Minute = 12,
    /// Based on time intervals with hour granularity.
    Hour = 13,
    /// Based on time intervals with day granularity.
    Day = 14,
    /// Based on time intervals with week granularity.
    Week = 15,
    /// Based on time intervals with month granularity.
    Month = 16,
}

/// The type of order book action for an order book event.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum BookAction {
    /// An order is added to the book.
    Add = 1,
    /// An existing order in the book is updated/modified.
    Update = 2,
    /// An existing order in the book is deleted/canceled.
    Delete = 3,
    /// The state of the order book is cleared.
    Clear = 4,
}

impl FromU8 for BookAction {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(BookAction::Add),
            2 => Some(BookAction::Update),
            3 => Some(BookAction::Delete),
            4 => Some(BookAction::Clear),
            _ => None,
        }
    }
}

/// The order book type, representing the type of levels granularity and delta updating heuristics.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
#[pyclass]
pub enum BookType {
    /// Top-of-book best bid/offer, one level per side.
    L1_TBBO = 1,
    /// Market by price, one order per level (aggregated).
    L2_MBP = 2,
    /// Market by order, multiple orders per level (full granularity).
    L3_MBO = 3,
}

impl FromU8 for BookType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(BookType::L1_TBBO),
            2 => Some(BookType::L2_MBP),
            3 => Some(BookType::L3_MBO),
            _ => None,
        }
    }
}

/// The order contigency type which specifies the behaviour of linked orders.
///
/// [FIX 5.0 SP2 : ContingencyType <1385> field](https://www.onixs.biz/fix-dictionary/5.0.sp2/tagnum_1385.html).
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum ContingencyType {
    /// Not a contingent order.
    NoContingency = 0, // Will be replaced by `Option`
    /// One-Cancels-the-Other.
    Oco = 1,
    /// One-Triggers-the-Other.
    Oto = 2,
    /// One-Updates-the-Other (by proportional quantity).
    Ouo = 3,
}

/// The broad currency type.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum CurrencyType {
    /// A type of cryptocurrency or crypto token.
    Crypto = 1,
    /// A type of currency issued by governments which is not backed by a commodity.
    Fiat = 2,
}

/// The type of event for an instrument close.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum InstrumentCloseType {
    /// When the market session ended.
    EndOfSession = 1,
    /// When the instrument expiration was reached.
    ContractExpired = 2,
}

/// The liqudity side for a trade in a financial market.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
#[pyclass]
pub enum LiquiditySide {
    /// No specific liqudity side.
    NoLiquiditySide = 0, // Will be replaced by `Option`
    /// The order passively provided liqudity to the market to complete the trade (made a market).
    Maker = 1,
    /// The order aggressively took liqudity from the market to complete the trade.
    Taker = 2,
}

/// The status of an individual market on a trading venue.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum MarketStatus {
    /// The market is closed.
    Closed = 1,
    /// The market is in the pre-open session.
    PreOpen = 2,
    /// The market is open for the normal session.
    Open = 3,
    /// The market session is paused.
    Pause = 4,
    /// The market is in the pre-close session.
    PreClose = 5,
}

/// The order management system (OMS) type for a trading venue or trading strategy.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum OmsType {
    /// There is no specific type of order management specified (will defer to the venue).
    Unspecified = 0, // Will be replaced by `Option`
    /// The netting type where there is one position per instrument.
    Netting = 1,
    /// The hedging type where there can be multiple positions per instrument.
    /// This can be in LONG/SHORT directions, by position/ticket ID, or tracked virtually by
    /// Nautilus.
    Hedging = 2,
}

/// The kind of options contract.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum OptionKind {
    /// A Call option gives the holder the right, but not the obligation, to buy an underlying asset at a specified strike price within a specified period of time.
    Call = 1,
    /// A Put option gives the holder the right, but not the obligation, to sell an underlying asset at a specified strike price within a specified period of time.
    Put = 2,
}

/// The order side for a specific order, or action related to orders.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
#[pyclass]
pub enum OrderSide {
    /// No order side is specified (only valid in the context of a filter for actions involving orders).
    NoOrderSide = 0, // Will be replaced by `Option`
    /// The order is a BUY.
    Buy = 1,
    /// The order is a SELL.
    Sell = 2,
}

/// Convert the given `value` to an [`OrderSide`].
impl FromU8 for OrderSide {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(OrderSide::NoOrderSide),
            1 => Some(OrderSide::Buy),
            2 => Some(OrderSide::Sell),
            _ => None,
        }
    }
}

/// The status for a specific order.
///
/// An order is considered _open_ for the following status:
///  - `ACCEPTED`
///  - `TRIGGERED`
///  - `PENDING_UPDATE`
///  - `PENDING_CANCEL`
///  - `PARTIALLY_FILLED`
///
/// An order is considered _in-flight_ for the following status:
///  - `SUBMITTED`
///  - `PENDING_UPDATE`
///  - `PENDING_CANCEL`
///
/// An order is considered _closed_ for the following status:
///  - `DENIED`
///  - `REJECTED`
///  - `CANCELED`
///  - `EXPIRED`
///  - `FILLED`
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum OrderStatus {
    /// The order is initialized (instantiated) within the Nautilus system.
    Initialized = 1,
    /// The order was denied by the Nautilus system, either for being invalid, unprocessable or exceeding a risk limit.
    Denied = 2,
    /// The order was submitted by the Nautilus system to the external service or trading venue (closed/done).
    Submitted = 3,
    /// The order was acknowledged by the trading venue as being received and valid (may now be working).
    Accepted = 4,
    /// The order was rejected by the trading venue.
    Rejected = 5,
    /// The order was canceled (closed/done).
    Canceled = 6,
    /// The order reached a GTD expiration (closed/done).
    Expired = 7,
    /// The order STOP price was triggered (closed/done).
    Triggered = 8,
    /// The order is currently pending a request to modify at the trading venue.
    PendingUpdate = 9,
    /// The order is currently pending a request to cancel at the trading venue.
    PendingCancel = 10,
    /// The order has been partially filled at the trading venue.
    PartiallyFilled = 11,
    /// The order has been completely filled at the trading venue (closed/done).
    Filled = 12,
}

/// The type of order.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum OrderType {
    /// A market order to buy or sell at the best available price in the current market.
    Market = 1,
    /// A limit order to buy or sell at a specific price or better.
    Limit = 2,
    /// A stop market order to buy or sell once the price reaches the specified stop/trigger price. When the stop price is reached, the order effectively becomes a market order.
    StopMarket = 3,
    /// A stop limit order to buy or sell which combines the features of a stop order and a limit order. Once the stop/trigger price is reached, a stop-limit order effectively becomes a limit order.
    StopLimit = 4,
    /// A market-to-limit order is a market order that is to be executed as a limit order at the current best market price after reaching the market.
    MarketToLimit = 5,
    /// A market-if-touched order effectively becomes a market order when the specified trigger price is reached.
    MarketIfTouched = 6,
    /// A limit-if-touched order effectively becomes a limit order when the specified trigger price is reached.
    LimitIfTouched = 7,
    /// A trailing stop market order sets the stop/trigger price at a fixed "trailing offset" amount from the market.
    TrailingStopMarket = 8,
    /// A trailing stop limit order combines the features of a trailing stop order with those of a limit order.
    TrailingStopLimit = 9,
}

/// The market side for a specific position, or action related to positions.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
#[pyclass]
pub enum PositionSide {
    /// No position side is specified (only valid in the context of a filter for actions involving positions).
    NoPositionSide = 0, // Will be replaced by `Option`
    /// A neural/flat position, where no position is currently held in the market.
    Flat = 1,
    /// A long position in the market, typically acquired through one or many BUY orders.
    Long = 2,
    /// A short position in the market, typically acquired through one or many SELL orders.
    Short = 3,
}

/// The type of price for an instrument in a financial market.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum PriceType {
    /// A quoted order price where a buyer is willing to buy a quantity of an instrument.
    Bid = 1,
    /// A quoted order price where a seller is willing to sell a quantity of an instrument.
    Ask = 2,
    /// The midpoint between the bid and ask prices.
    Mid = 3,
    /// The last price at which a trade was made for an instrument.
    Last = 4,
}

/// The 'Time in Force' instruction for an order in the financial market.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum TimeInForce {
    /// Good Till Canceled (GTC) - the order remains active until canceled.
    Gtc = 1,
    /// Immediate or Cancel (IOC) - the order is filled as much as possible, the rest is canceled.
    Ioc = 2,
    /// Fill or Kill (FOK) - the order must be executed in full immediately, or it is canceled.
    Fok = 3,
    /// Good Till Date/Time (GTD) - the order is active until a specified date or time.
    Gtd = 4,
    /// Day - the order is active until the end of the current trading session.
    Day = 5,
    /// At the Opening (ATO) - the order is scheduled to be executed at the market's opening.
    AtTheOpen = 6,
    /// At the Closing (ATC) - the order is scheduled to be executed at the market's closing.
    AtTheClose = 7,
}

/// The trading state for a node.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum TradingState {
    /// Normal trading operations.
    Active = 1,
    /// Trading is completely halted, no new order commands will be emitted.
    Halted = 2,
    /// Only order commands which would cancel order, or reduce position sizes are permitted.
    Reducing = 3,
}

/// The trailing offset type for an order type which specifies a trailing stop/trigger or limit price.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum TrailingOffsetType {
    /// No trailing offset type is specified (invalid for trailing type orders).
    NoTrailingOffset = 0, // Will be replaced by `Option`
    /// The trailing offset is based on a market price.
    Price = 1,
    /// The trailing offset is based on a percentage represented in basis points, of a market price.
    BasisPoints = 2,
    /// The trailing offset is based on the number of ticks from a market price.
    Ticks = 3,
    /// The trailing offset is based on a price tier set by a specific trading venue.
    PriceTier = 4,
}

/// The trigger type for the stop/trigger price of an order.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass]
pub enum TriggerType {
    /// No trigger type is specified (invalid for orders with a trigger).
    NoTrigger = 0, // Will be replaced by `Option`
    /// The default trigger type set by the trading venue.
    Default = 1,
    /// Based on the top-of-book quoted prices for the instrument.
    BidAsk = 2,
    /// Based on the last traded price for the instrument.
    LastTrade = 3,
    /// Based on a 'double match' of the last traded price for the instrument
    DoubleLast = 4,
    /// Based on a 'double match' of the bid/ask price for the instrument
    DoubleBidAsk = 5,
    /// Based on both the [`TriggerType::LastTrade`] and [`TriggerType::BidAsk`].
    LastOrBidAsk = 6,
    /// Based on the mid-point of the [`TriggerType::BidAsk`].
    MidPoint = 7,
    /// Based on the mark price for the instrument.
    MarkPrice = 8,
    /// Based on the index price for the instrument.
    IndexPrice = 9,
}

strum_serde!(AccountType);
strum_serde!(AggregationSource);
strum_serde!(AggressorSide);
strum_serde!(AssetClass);
strum_serde!(AssetType);
strum_serde!(BarAggregation);
strum_serde!(BookAction);
strum_serde!(BookType);
strum_serde!(ContingencyType);
strum_serde!(CurrencyType);
strum_serde!(InstrumentCloseType);
strum_serde!(LiquiditySide);
strum_serde!(MarketStatus);
strum_serde!(OmsType);
strum_serde!(OptionKind);
strum_serde!(OrderSide);
strum_serde!(OrderStatus);
strum_serde!(OrderType);
strum_serde!(PositionSide);
strum_serde!(PriceType);
strum_serde!(TimeInForce);
strum_serde!(TradingState);
strum_serde!(TrailingOffsetType);
strum_serde!(TriggerType);

#[no_mangle]
pub extern "C" fn account_type_to_cstr(value: AccountType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn account_type_from_cstr(ptr: *const c_char) -> AccountType {
    let value = cstr_to_string(ptr);
    AccountType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AccountType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn aggregation_source_to_cstr(value: AggregationSource) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn aggregation_source_from_cstr(ptr: *const c_char) -> AggregationSource {
    let value = cstr_to_string(ptr);
    AggregationSource::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AggregationSource` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn aggressor_side_to_cstr(value: AggressorSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn aggressor_side_from_cstr(ptr: *const c_char) -> AggressorSide {
    let value = cstr_to_string(ptr);
    AggressorSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AggressorSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn asset_class_to_cstr(value: AssetClass) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn asset_class_from_cstr(ptr: *const c_char) -> AssetClass {
    let value = cstr_to_string(ptr);
    AssetClass::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AssetClass` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn asset_type_to_cstr(value: AssetType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn asset_type_from_cstr(ptr: *const c_char) -> AssetType {
    let value = cstr_to_string(ptr);
    AssetType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AssetType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn bar_aggregation_to_cstr(value: BarAggregation) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn bar_aggregation_from_cstr(ptr: *const c_char) -> BarAggregation {
    let value = cstr_to_string(ptr);
    BarAggregation::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BarAggregation` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn book_action_to_cstr(value: BookAction) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn book_action_from_cstr(ptr: *const c_char) -> BookAction {
    let value = cstr_to_string(ptr);
    BookAction::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BookAction` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn book_type_to_cstr(value: BookType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn book_type_from_cstr(ptr: *const c_char) -> BookType {
    let value = cstr_to_string(ptr);
    BookType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BookType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn contingency_type_to_cstr(value: ContingencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn contingency_type_from_cstr(ptr: *const c_char) -> ContingencyType {
    let value = cstr_to_string(ptr);
    ContingencyType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `ContingencyType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn currency_type_to_cstr(value: CurrencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn currency_type_from_cstr(ptr: *const c_char) -> CurrencyType {
    let value = cstr_to_string(ptr);
    CurrencyType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `CurrencyType` enum string value, was '{value}'"))
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn instrument_close_type_from_cstr(
    ptr: *const c_char,
) -> InstrumentCloseType {
    let value = cstr_to_string(ptr);
    InstrumentCloseType::from_str(&value).unwrap_or_else(|_| {
        panic!("invalid `InstrumentCloseType` enum string value, was '{value}'")
    })
}

#[no_mangle]
pub extern "C" fn instrument_close_type_to_cstr(value: InstrumentCloseType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

#[no_mangle]
pub extern "C" fn liquidity_side_to_cstr(value: LiquiditySide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn liquidity_side_from_cstr(ptr: *const c_char) -> LiquiditySide {
    let value = cstr_to_string(ptr);
    LiquiditySide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `LiquiditySide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn market_status_to_cstr(value: MarketStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn market_status_from_cstr(ptr: *const c_char) -> MarketStatus {
    let value = cstr_to_string(ptr);
    MarketStatus::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `MarketStatus` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn oms_type_to_cstr(value: OmsType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn oms_type_from_cstr(ptr: *const c_char) -> OmsType {
    let value = cstr_to_string(ptr);
    OmsType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OmsType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn option_kind_to_cstr(value: OptionKind) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn option_kind_from_cstr(ptr: *const c_char) -> OptionKind {
    let value = cstr_to_string(ptr);
    OptionKind::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OptionKind` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_side_to_cstr(value: OrderSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_side_from_cstr(ptr: *const c_char) -> OrderSide {
    let value = cstr_to_string(ptr);
    OrderSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_status_to_cstr(value: OrderStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_status_from_cstr(ptr: *const c_char) -> OrderStatus {
    let value = cstr_to_string(ptr);
    OrderStatus::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderStatus` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn order_type_to_cstr(value: OrderType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn order_type_from_cstr(ptr: *const c_char) -> OrderType {
    let value = cstr_to_string(ptr);
    OrderType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn position_side_to_cstr(value: PositionSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn position_side_from_cstr(ptr: *const c_char) -> PositionSide {
    let value = cstr_to_string(ptr);
    PositionSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `PositionSide` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn price_type_to_cstr(value: PriceType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn price_type_from_cstr(ptr: *const c_char) -> PriceType {
    let value = cstr_to_string(ptr);
    PriceType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `PriceType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn time_in_force_to_cstr(value: TimeInForce) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn time_in_force_from_cstr(ptr: *const c_char) -> TimeInForce {
    let value = cstr_to_string(ptr);
    TimeInForce::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TimeInForce` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trading_state_to_cstr(value: TradingState) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trading_state_from_cstr(ptr: *const c_char) -> TradingState {
    let value = cstr_to_string(ptr);
    TradingState::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TradingState` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trailing_offset_type_to_cstr(value: TrailingOffsetType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trailing_offset_type_from_cstr(ptr: *const c_char) -> TrailingOffsetType {
    let value = cstr_to_string(ptr);
    TrailingOffsetType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TrailingOffsetType` enum string value, was '{value}'"))
}

#[no_mangle]
pub extern "C" fn trigger_type_to_cstr(value: TriggerType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn trigger_type_from_cstr(ptr: *const c_char) -> TriggerType {
    let value = cstr_to_string(ptr);
    TriggerType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TriggerType` enum string value, was '{value}'"))
}
