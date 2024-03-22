// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};

use crate::enum_strum_serde;

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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum AggressorSide {
    /// There was no specific aggressor for the trade.
    NoAggressor = 0,
    /// The BUY order was the aggressor for the trade.
    Buyer = 1,
    /// The SELL order was the aggressor for the trade.
    Seller = 2,
}

impl FromU8 for AggressorSide {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::NoAggressor),
            1 => Some(Self::Buyer),
            2 => Some(Self::Seller),
            _ => None,
        }
    }
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
#[allow(non_camel_case_types)]
pub enum AssetClass {
    /// Foreign exchange (FOREX) assets.
    FX = 1,
    /// Equity / stock assets.
    Equity = 2,
    /// Commodity assets.
    Commodity = 3,
    /// Debt based assets.
    Debt = 4,
    /// Index based assets (baskets).
    Index = 5,
    /// Cryptocurrency or crypto token assets.
    Cryptocurrency = 6,
    /// Alternative assets.
    Alternative = 7,
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum InstrumentClass {
    /// A spot market instrument class. The current market price of an instrument that is bought or sold for immediate delivery and payment.
    Spot = 1,
    /// A swap instrument class. A derivative contract through which two parties exchange the cash flows or liabilities from two different financial instruments.
    Swap = 2,
    /// A futures contract instrument class. A legal agreement to buy or sell an asset at a predetermined price at a specified time in the future.
    Future = 3,
    /// A futures spread instrument class. A strategy involving the use of futures contracts to take advantage of price differentials between different contract months, underlying assets, or marketplaces.
    FutureSpread = 4,
    /// A forward derivative instrument class. A customized contract between two parties to buy or sell an asset at a specified price on a future date.
    Forward = 5,
    /// A contract-for-difference (CFD) instrument class. A contract between an investor and a CFD broker to exchange the difference in the value of a financial product between the time the contract opens and closes.
    Cfd = 6,
    /// A bond instrument class. A type of debt investment where an investor loans money to an entity (typically corporate or governmental) which borrows the funds for a defined period of time at a variable or fixed interest rate.
    Bond = 7,
    /// An options contract instrument class. A type of derivative that gives the holder the right, but not the obligation, to buy or sell an underlying asset at a predetermined price before or at a certain future date.
    Option = 8,
    /// An option spread instrument class. A strategy involving the purchase and/or sale of options on the same underlying asset with different strike prices or expiration dates to capitalize on expected market moves in a controlled cost environment.
    OptionSpread = 9,
    /// A warrant instrument class. A derivative that gives the holder the right, but not the obligation, to buy or sell a security—most commonly an equity—at a certain price before expiration.
    Warrant = 10,
    /// A warrant instrument class. A derivative that gives the holder the right, but not the obligation, to buy or sell a security—most commonly an equity—at a certain price before expiration.
    SportsBetting = 11,
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
            1 => Some(Self::Add),
            2 => Some(Self::Update),
            3 => Some(Self::Delete),
            4 => Some(Self::Clear),
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum BookType {
    /// Top-of-book best bid/ask, one level per side.
    L1_MBP = 1,
    /// Market by price, one order per level (aggregated).
    L2_MBP = 2,
    /// Market by order, multiple orders per level (full granularity).
    L3_MBO = 3,
}

impl FromU8 for BookType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::L1_MBP),
            2 => Some(Self::L2_MBP),
            3 => Some(Self::L3_MBO),
            _ => None,
        }
    }
}

/// The order contigency type which specifies the behavior of linked orders.
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum CurrencyType {
    /// A type of cryptocurrency or crypto token.
    Crypto = 1,
    /// A type of currency issued by governments which is not backed by a commodity.
    Fiat = 2,
    /// A type of currency that is based on the value of an underlying commodity.
    CommodityBacked = 3,
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
#[allow(clippy::enum_variant_names)]
pub enum LiquiditySide {
    /// No liquidity side specified.
    NoLiquiditySide = 0,
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum MarketStatus {
    /// The market session is in the pre-open.
    PreOpen = 1,
    /// The market session is open.
    Open = 2,
    /// The market session is paused.
    Pause = 3,
    /// The market session is halted.
    Halt = 4,
    /// The market session has reopened after a pause or halt.
    Reopen = 5,
    /// The market session is in the pre-close.
    PreClose = 6,
    /// The market session is closed.
    Closed = 7,
}

/// The reason for a venue or market halt.
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum HaltReason {
    /// The venue or market session is not halted.
    NotHalted = 1,
    /// Trading halt is imposed for purely regulatory reasons with/without volatility halt.
    General = 2,
    /// Trading halt is imposed by the venue to protect against extreme volatility.
    Volatility = 3,
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum OrderSide {
    /// No order side is specified.
    NoOrderSide = 0,
    /// The order is a BUY.
    Buy = 1,
    /// The order is a SELL.
    Sell = 2,
}

/// Convert the given `value` to an [`OrderSide`].
impl FromU8 for OrderSide {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::NoOrderSide),
            1 => Some(Self::Buy),
            2 => Some(Self::Sell),
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
pub enum OrderStatus {
    /// The order is initialized (instantiated) within the Nautilus system.
    Initialized = 1,
    /// The order was denied by the Nautilus system, either for being invalid, unprocessable or exceeding a risk limit.
    Denied = 2,
    /// The order became emulated by the Nautilus system in the `OrderEmulator` component.
    Emulated = 3,
    /// The order was released by the Nautilus system from the `OrderEmulator` component.
    Released = 4,
    /// The order was submitted by the Nautilus system to the external service or trading venue (awaiting acknowledgement).
    Submitted = 5,
    /// The order was acknowledged by the trading venue as being received and valid (may now be working).
    Accepted = 6,
    /// The order was rejected by the trading venue.
    Rejected = 7,
    /// The order was canceled (closed/done).
    Canceled = 8,
    /// The order reached a GTD expiration (closed/done).
    Expired = 9,
    /// The order STOP price was triggered on a trading venue.
    Triggered = 10,
    /// The order is currently pending a request to modify on a trading venue.
    PendingUpdate = 11,
    /// The order is currently pending a request to cancel on a trading venue.
    PendingCancel = 12,
    /// The order has been partially filled on a trading venue.
    PartiallyFilled = 13,
    /// The order has been completely filled on a trading venue (closed/done).
    Filled = 14,
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")
)]
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

enum_strum_serde!(AccountType);
enum_strum_serde!(AggregationSource);
enum_strum_serde!(AggressorSide);
enum_strum_serde!(AssetClass);
enum_strum_serde!(InstrumentClass);
enum_strum_serde!(BarAggregation);
enum_strum_serde!(BookAction);
enum_strum_serde!(BookType);
enum_strum_serde!(ContingencyType);
enum_strum_serde!(CurrencyType);
enum_strum_serde!(InstrumentCloseType);
enum_strum_serde!(LiquiditySide);
enum_strum_serde!(MarketStatus);
enum_strum_serde!(OmsType);
enum_strum_serde!(OptionKind);
enum_strum_serde!(OrderSide);
enum_strum_serde!(OrderStatus);
enum_strum_serde!(OrderType);
enum_strum_serde!(PositionSide);
enum_strum_serde!(PriceType);
enum_strum_serde!(TimeInForce);
enum_strum_serde!(TradingState);
enum_strum_serde!(TrailingOffsetType);
enum_strum_serde!(TriggerType);
