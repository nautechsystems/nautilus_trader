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

//! Enumerations for the trading domain model.

use std::{str::FromStr, sync::OnceLock};

use ahash::AHashSet;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};

use crate::enum_strum_serde;

/// Provides conversion from a `u8` value to an enum type.
pub trait FromU8 {
    /// Converts a `u8` value to the implementing type.
    ///
    /// Returns `None` if the value is not a valid representation.
    fn from_u8(value: u8) -> Option<Self>
    where
        Self: Sized;
}

/// Provides conversion from a `u16` value to an enum type.
pub trait FromU16 {
    /// Converts a `u16` value to the implementing type.
    ///
    /// Returns `None` if the value is not a valid representation.
    fn from_u16(value: u16) -> Option<Self>
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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
    Default,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum AggressorSide {
    /// There was no specific aggressor for the trade.
    #[default]
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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

impl FromU8 for AssetClass {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::FX),
            2 => Some(Self::Equity),
            3 => Some(Self::Commodity),
            4 => Some(Self::Debt),
            5 => Some(Self::Index),
            6 => Some(Self::Cryptocurrency),
            7 => Some(Self::Alternative),
            _ => None,
        }
    }
}

/// The instrument class.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum InstrumentClass {
    /// A spot market instrument class. The current market price of an instrument that is bought or sold for immediate delivery and payment.
    Spot = 1,
    /// A swap instrument class. A derivative contract through which two parties exchange the cash flows or liabilities from two different financial instruments.
    Swap = 2,
    /// A futures contract instrument class. A legal agreement to buy or sell an asset at a predetermined price at a specified time in the future.
    Future = 3,
    /// A futures spread instrument class. A strategy involving the use of futures contracts to take advantage of price differentials between different contract months, underlying assets, or marketplaces.
    FuturesSpread = 4,
    /// A forward derivative instrument class. A customized contract between two parties to buy or sell an asset at a specified price on a future date.
    Forward = 5,
    /// A contract-for-difference (CFD) instrument class. A contract between an investor and a CFD broker to exchange the difference in the value of a financial product between the time the contract opens and closes.
    Cfd = 6,
    /// A bond instrument class. A type of debt investment where an investor loans money to an entity (typically corporate or governmental) which borrows the funds for a defined period of time at a variable or fixed interest rate.
    Bond = 7,
    /// An option contract instrument class. A type of derivative that gives the holder the right, but not the obligation, to buy or sell an underlying asset at a predetermined price before or at a certain future date.
    Option = 8,
    /// An option spread instrument class. A strategy involving the purchase and/or sale of multiple option contracts on the same underlying asset with different strike prices or expiration dates to hedge risk or speculate on price movements.
    OptionSpread = 9,
    /// A warrant instrument class. A derivative that gives the holder the right, but not the obligation, to buy or sell a security—most commonly an equity—at a certain price before expiration.
    Warrant = 10,
    /// A sports betting instrument class. A financialized derivative that allows wagering on the outcome of sports events using structured contracts or prediction markets.
    SportsBetting = 11,
    /// A binary option instrument class. A type of derivative where the payoff is either a fixed monetary amount or nothing, depending on whether the price of an underlying asset is above or below a predetermined level at expiration.
    /// A binary option instrument class. A type of derivative where the payoff is either a fixed monetary amount or nothing, based on a yes/no proposition about an underlying event.
    BinaryOption = 12,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum BarAggregation {
    /// Based on a number of ticks.
    Tick = 1,
    /// Based on the buy/sell imbalance of ticks.
    TickImbalance = 2,
    /// Based on sequential buy/sell runs of ticks.
    TickRuns = 3,
    /// Based on traded volume.
    Volume = 4,
    /// Based on the buy/sell imbalance of traded volume.
    VolumeImbalance = 5,
    /// Based on sequential runs of buy/sell traded volume.
    VolumeRuns = 6,
    /// Based on the 'notional' value of the instrument.
    Value = 7,
    /// Based on the buy/sell imbalance of trading by notional value.
    ValueImbalance = 8,
    /// Based on sequential buy/sell runs of trading by notional value.
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
    /// Based on time intervals with year granularity.
    Year = 17,
    /// Based on fixed price movements (brick size).
    Renko = 18,
}

/// The interval type for bar aggregation.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum BarIntervalType {
    /// Left-open interval `(start, end]`: start is exclusive, end is inclusive (default).
    #[default]
    LeftOpen = 1,
    /// Right-open interval `[start, end)`: start is inclusive, end is exclusive.
    RightOpen = 2,
}

/// Represents the side of a bet in a betting market.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum BetSide {
    /// A "Back" bet signifies support for a specific outcome.
    Back = 1,
    /// A "Lay" bet signifies opposition to a specific outcome.
    Lay = 2,
}

impl BetSide {
    /// Returns the opposite betting side.
    #[must_use]
    pub fn opposite(&self) -> Self {
        match self {
            Self::Back => Self::Lay,
            Self::Lay => Self::Back,
        }
    }
}

impl From<OrderSide> for BetSide {
    /// Returns the equivalent [`BetSide`] for a given [`OrderSide`].
    ///
    /// # Panics
    ///
    /// Panics if `side` is [`OrderSide::NoOrderSide`].
    fn from(side: OrderSide) -> Self {
        match side {
            OrderSide::Buy => Self::Back,
            OrderSide::Sell => Self::Lay,
            OrderSide::NoOrderSide => panic!("Invalid `OrderSide` for `BetSide`, was {side}"),
        }
    }
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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

/// The order contingency type which specifies the behavior of linked orders.
///
/// [FIX 5.0 SP2 : ContingencyType <1385> field](https://www.onixs.biz/fix-dictionary/5.0.sp2/tagnum_1385.html).
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum ContingencyType {
    /// Not a contingent order.
    #[default]
    NoContingency = 0,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum InstrumentCloseType {
    /// When the market session ended.
    EndOfSession = 1,
    /// When the instrument expiration was reached.
    ContractExpired = 2,
}

/// Convert the given `value` to an [`InstrumentCloseType`].
impl FromU8 for InstrumentCloseType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::EndOfSession),
            2 => Some(Self::ContractExpired),
            _ => None,
        }
    }
}

/// The liquidity side for a trade.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
#[allow(clippy::enum_variant_names)]
pub enum LiquiditySide {
    /// No liquidity side specified.
    NoLiquiditySide = 0,
    /// The order passively provided liquidity to the market to complete the trade (made a market).
    Maker = 1,
    /// The order aggressively took liquidity from the market to complete the trade.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum MarketStatus {
    /// The instrument is trading.
    Open = 1,
    /// The instrument is in a pre-open period.
    Closed = 2,
    /// Trading in the instrument has been paused.
    Paused = 3,
    /// Trading in the instrument has been halted.
    // Halted = 4,  # TODO: Unfortunately can't use this yet due to Cython (C enum namespacing)
    /// Trading in the instrument has been suspended.
    Suspended = 5,
    /// Trading in the instrument is not available.
    NotAvailable = 6,
}

/// An action affecting the status of an individual market on a trading venue.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum MarketStatusAction {
    /// No change.
    None = 0,
    /// The instrument is in a pre-open period.
    PreOpen = 1,
    /// The instrument is in a pre-cross period.
    PreCross = 2,
    /// The instrument is quoting but not trading.
    Quoting = 3,
    /// The instrument is in a cross/auction.
    Cross = 4,
    /// The instrument is being opened through a trading rotation.
    Rotation = 5,
    /// A new price indication is available for the instrument.
    NewPriceIndication = 6,
    /// The instrument is trading.
    Trading = 7,
    /// Trading in the instrument has been halted.
    Halt = 8,
    /// Trading in the instrument has been paused.
    Pause = 9,
    /// Trading in the instrument has been suspended.
    Suspend = 10,
    /// The instrument is in a pre-close period.
    PreClose = 11,
    /// Trading in the instrument has closed.
    Close = 12,
    /// The instrument is in a post-close period.
    PostClose = 13,
    /// A change in short-selling restrictions.
    ShortSellRestrictionChange = 14,
    /// The instrument is not available for trading, either trading has closed or been halted.
    NotAvailableForTrading = 15,
}

/// Convert the given `value` to an [`OrderSide`].
impl FromU16 for MarketStatusAction {
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::PreOpen),
            2 => Some(Self::PreCross),
            3 => Some(Self::Quoting),
            4 => Some(Self::Cross),
            5 => Some(Self::Rotation),
            6 => Some(Self::NewPriceIndication),
            7 => Some(Self::Trading),
            8 => Some(Self::Halt),
            9 => Some(Self::Pause),
            10 => Some(Self::Suspend),
            11 => Some(Self::PreClose),
            12 => Some(Self::Close),
            13 => Some(Self::PostClose),
            14 => Some(Self::ShortSellRestrictionChange),
            15 => Some(Self::NotAvailableForTrading),
            _ => None,
        }
    }
}

/// The order management system (OMS) type for a trading venue or trading strategy.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum OmsType {
    /// There is no specific type of order management specified (will defer to the venue OMS).
    #[default]
    Unspecified = 0,
    /// The netting type where there is one position per instrument.
    Netting = 1,
    /// The hedging type where there can be multiple positions per instrument.
    /// This can be in LONG/SHORT directions, by position/ticket ID, or tracked virtually by
    /// Nautilus.
    Hedging = 2,
}

/// The kind of option contract.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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
    Default,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum OrderSide {
    /// No order side is specified.
    #[default]
    NoOrderSide = 0,
    /// The order is a BUY.
    Buy = 1,
    /// The order is a SELL.
    Sell = 2,
}

impl OrderSide {
    /// Returns the specified [`OrderSideSpecified`] (BUY or SELL) for this side.
    ///
    /// # Panics
    ///
    /// Panics if `self` is [`OrderSide::NoOrderSide`].
    #[must_use]
    pub fn as_specified(&self) -> OrderSideSpecified {
        match &self {
            Self::Buy => OrderSideSpecified::Buy,
            Self::Sell => OrderSideSpecified::Sell,
            _ => panic!("Order invariant failed: side must be `Buy` or `Sell`"),
        }
    }
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

/// The specified order side (BUY or SELL).
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
pub enum OrderSideSpecified {
    /// The order is a BUY.
    Buy = 1,
    /// The order is a SELL.
    Sell = 2,
}

impl OrderSideSpecified {
    /// Returns the opposite order side.
    #[must_use]
    pub fn opposite(&self) -> Self {
        match &self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }

    /// Converts this specified side into an [`OrderSide`].
    #[must_use]
    pub fn as_order_side(&self) -> OrderSide {
        match &self {
            Self::Buy => OrderSide::Buy,
            Self::Sell => OrderSide::Sell,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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

impl OrderStatus {
    /// Returns a cached `AHashSet` of order statuses safe for cancellation queries.
    ///
    /// These are statuses where an order is working on the venue but not already
    /// in the process of being cancelled or updated. Including `PENDING_CANCEL`
    /// in cancellation filters can cause duplicate cancel attempts or incorrect open order counts.
    ///
    /// Returns:
    /// - `ACCEPTED`: Order is working on the venue.
    /// - `TRIGGERED`: Stop order has been triggered.
    /// - `PENDING_UPDATE`: Order being updated.
    /// - `PARTIALLY_FILLED`: Order is partially filled but still working.
    ///
    /// Excludes:
    /// - `PENDING_CANCEL`: Already being cancelled.
    #[must_use]
    pub fn cancellable_statuses_set() -> &'static AHashSet<Self> {
        static CANCELLABLE_SET: OnceLock<AHashSet<OrderStatus>> = OnceLock::new();
        CANCELLABLE_SET.get_or_init(|| {
            AHashSet::from_iter([
                Self::Accepted,
                Self::Triggered,
                Self::PendingUpdate,
                Self::PartiallyFilled,
            ])
        })
    }
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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
    Default,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum PositionSide {
    /// No position side is specified (only valid in the context of a filter for actions involving positions).
    #[default]
    NoPositionSide = 0,
    /// A neural/flat position, where no position is currently held in the market.
    Flat = 1,
    /// A long position in the market, typically acquired through one or many BUY orders.
    Long = 2,
    /// A short position in the market, typically acquired through one or many SELL orders.
    Short = 3,
}

impl PositionSide {
    /// Returns the specified [`PositionSideSpecified`] (`Long`, `Short`, or `Flat`) for this side.
    ///
    /// # Panics
    ///
    /// Panics if `self` is [`PositionSide::NoPositionSide`].
    #[must_use]
    pub fn as_specified(&self) -> PositionSideSpecified {
        match &self {
            Self::Long => PositionSideSpecified::Long,
            Self::Short => PositionSideSpecified::Short,
            Self::Flat => PositionSideSpecified::Flat,
            _ => panic!("Position invariant failed: side must be `Long`, `Short`, or `Flat`"),
        }
    }
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum PositionSideSpecified {
    /// A neural/flat position, where no position is currently held in the market.
    Flat = 1,
    /// A long position in the market, typically acquired through one or many BUY orders.
    Long = 2,
    /// A short position in the market, typically acquired through one or many SELL orders.
    Short = 3,
}

impl PositionSideSpecified {
    /// Converts this specified side into a [`PositionSide`].
    #[must_use]
    pub fn as_position_side(&self) -> PositionSide {
        match &self {
            Self::Long => PositionSide::Long,
            Self::Short => PositionSide::Short,
            Self::Flat => PositionSide::Flat,
        }
    }
}

/// The type of price for an instrument in a market.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum PriceType {
    /// The best quoted price at which buyers are willing to buy a quantity of an instrument.
    /// Often considered the best bid in the order book.
    Bid = 1,
    /// The best quoted price at which sellers are willing to sell a quantity of an instrument.
    /// Often considered the best ask in the order book.
    Ask = 2,
    /// The arithmetic midpoint between the best bid and ask quotes.
    Mid = 3,
    /// The price at which the last trade of an instrument was executed.
    Last = 4,
    /// A reference price reflecting an instrument's fair value, often used for portfolio
    /// calculations and risk management.
    Mark = 5,
}

/// A record flag bit field, indicating event end and data information.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
#[allow(non_camel_case_types)]
pub enum RecordFlag {
    /// Last message in the book event or packet from the venue for a given `instrument_id`.
    F_LAST = 1 << 7, // 128
    /// Top-of-book message, not an individual order.
    F_TOB = 1 << 6, // 64
    /// Message sourced from a replay, such as a snapshot server.
    F_SNAPSHOT = 1 << 5, // 32
    /// Aggregated price level message, not an individual order.
    F_MBP = 1 << 4, // 16
    /// Reserved for future use.
    RESERVED_2 = 1 << 3, // 8
    /// Reserved for future use.
    RESERVED_1 = 1 << 2, // 4
}

impl RecordFlag {
    /// Checks if the flag matches a given value.
    #[must_use]
    pub fn matches(self, value: u8) -> bool {
        (self as u8) & value != 0
    }
}

/// The 'Time in Force' instruction for an order.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum TimeInForce {
    /// Good Till Cancel (GTC) - Remains active until canceled.
    Gtc = 1,
    /// Immediate or Cancel (IOC) - Executes immediately to the extent possible, with any unfilled portion canceled.
    Ioc = 2,
    /// Fill or Kill (FOK) - Executes in its entirety immediately or is canceled if full execution is not possible.
    Fok = 3,
    /// Good Till Date (GTD) - Remains active until the specified expiration date or time is reached.
    Gtd = 4,
    /// Day - Remains active until the close of the current trading session.
    Day = 5,
    /// At the Opening (ATO) - Executes at the market opening or expires if not filled.
    AtTheOpen = 6,
    /// At the Closing (ATC) - Executes at the market close or expires if not filled.
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
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
    Default,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum TrailingOffsetType {
    /// No trailing offset type is specified (invalid for trailing type orders).
    #[default]
    NoTrailingOffset = 0,
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
    Default,
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
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.model.enums"
    )
)]
pub enum TriggerType {
    /// No trigger type is specified (invalid for orders with a trigger).
    #[default]
    NoTrigger = 0,
    /// The default trigger type set by the trading venue.
    Default = 1,
    /// Based on the last traded price for the instrument.
    LastPrice = 2,
    /// Based on the mark price for the instrument.
    MarkPrice = 3,
    /// Based on the index price for the instrument.
    IndexPrice = 4,
    /// Based on the top-of-book quoted prices for the instrument.
    BidAsk = 5,
    /// Based on a 'double match' of the last traded price for the instrument
    DoubleLast = 6,
    /// Based on a 'double match' of the bid/ask price for the instrument
    DoubleBidAsk = 7,
    /// Based on both the [`TriggerType::LastPrice`] and [`TriggerType::BidAsk`].
    LastOrBidAsk = 8,
    /// Based on the mid-point of the [`TriggerType::BidAsk`].
    MidPoint = 9,
}

enum_strum_serde!(AccountType);
enum_strum_serde!(AggregationSource);
enum_strum_serde!(AggressorSide);
enum_strum_serde!(AssetClass);
enum_strum_serde!(InstrumentClass);
enum_strum_serde!(BarAggregation);
enum_strum_serde!(BarIntervalType);
enum_strum_serde!(BookAction);
enum_strum_serde!(BookType);
enum_strum_serde!(ContingencyType);
enum_strum_serde!(CurrencyType);
enum_strum_serde!(InstrumentCloseType);
enum_strum_serde!(LiquiditySide);
enum_strum_serde!(MarketStatus);
enum_strum_serde!(MarketStatusAction);
enum_strum_serde!(OmsType);
enum_strum_serde!(OptionKind);
enum_strum_serde!(OrderSide);
enum_strum_serde!(OrderSideSpecified);
enum_strum_serde!(OrderStatus);
enum_strum_serde!(OrderType);
enum_strum_serde!(PositionSide);
enum_strum_serde!(PositionSideSpecified);
enum_strum_serde!(PriceType);
enum_strum_serde!(RecordFlag);
enum_strum_serde!(TimeInForce);
enum_strum_serde!(TradingState);
enum_strum_serde!(TrailingOffsetType);
enum_strum_serde!(TriggerType);
