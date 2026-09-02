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

//! Enumerations for the trading domain model.

use std::{borrow::Cow, fmt::Display, marker::PhantomData, str::FromStr};

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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum AccountType {
    /// An account with unleveraged cash assets only.
    Cash = 1,
    /// An account which facilitates trading on margin, using account assets as collateral.
    Margin = 2,
    /// An account specific to betting markets.
    Betting = 3,
    /// An account which represents a blockchain wallet,
    Wallet = 4,
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum AggressorSide {
    /// There was no specific aggressor for the trade.
    #[default]
    NoAggressor = 0,
    /// The BUY order was the aggressor for the trade.
    ///
    /// The deprecated `BUYER` serialization value is still accepted when parsing.
    #[strum(serialize = "BUYER", to_string = "BUY")]
    Buy = 1,
    /// The SELL order was the aggressor for the trade.
    ///
    /// The deprecated `SELLER` serialization value is still accepted when parsing.
    #[strum(serialize = "SELLER", to_string = "SELL")]
    Sell = 2,
}

impl FromU8 for AggressorSide {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::NoAggressor),
            1 => Some(Self::Buy),
            2 => Some(Self::Sell),
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
    fn from(side: OrderSide) -> Self {
        match side {
            OrderSide::Buy => Self::Back,
            OrderSide::Sell => Self::Lay,
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
#[allow(non_camel_case_types)]
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
///
/// Python retains `NO_CONTINGENCY` as a compatibility alias for `None`. The alias is not an enum
/// variant and may be removed in a future version.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum ContingencyType {
    /// One-Cancels-the-Other.
    Oco = 1,
    /// One-Triggers-the-Other.
    Oto = 2,
    /// One-Updates-the-Other (by proportional quantity).
    Ouo = 3,
}

/// The price-adjustment scheme applied when stitching segment contracts into a
/// continuous future series.
///
/// The direction (backward vs. forward) selects the anchor contract:
/// - Backward modes anchor on the most recent contract; prices in older
///   segments are shifted into the latest contract's frame.
/// - Forward modes anchor on the first contract; prices in later segments
///   are shifted into the first contract's frame.
///
/// The kind (spread vs. ratio) selects how each transition's offset is combined:
/// - Spread modes accumulate additive offsets (`post_price - pre_price`).
/// - Ratio modes accumulate multiplicative factors (`post_price / pre_price`)
///   and require strictly positive prices.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum ContinuousFutureAdjustmentType {
    /// Additive adjustment, anchored on the most recent contract.
    #[default]
    BackwardSpread = 1,
    /// Additive adjustment, anchored on the first contract.
    ForwardSpread = 2,
    /// Multiplicative adjustment, anchored on the most recent contract.
    BackwardRatio = 3,
    /// Multiplicative adjustment, anchored on the first contract.
    ForwardRatio = 4,
}

impl ContinuousFutureAdjustmentType {
    /// Returns whether this mode accumulates multiplicative factors.
    #[must_use]
    pub const fn is_ratio(&self) -> bool {
        matches!(self, Self::BackwardRatio | Self::ForwardRatio)
    }

    /// Returns whether this mode anchors on the most recent contract.
    #[must_use]
    pub const fn is_backward(&self) -> bool {
        matches!(self, Self::BackwardSpread | Self::BackwardRatio)
    }
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum CurrencyType {
    /// A type of cryptocurrency or crypto token.
    Crypto = 1,
    /// A type of currency issued by governments which is not backed by a commodity.
    Fiat = 2,
    /// A type of currency that is based on the value of an underlying commodity.
    CommodityBacked = 3,
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
    /// A warrant instrument class. A derivative that gives the holder the right, but not the obligation, to buy or sell a security - most commonly an equity - at a certain price before expiration.
    Warrant = 10,
    /// A sports betting instrument class. A financialized derivative that allows wagering on the outcome of sports events using structured contracts or prediction markets.
    SportsBetting = 11,
    /// A binary option instrument class. A type of derivative where the payoff is either a fixed monetary amount or nothing, depending on whether the price of an underlying asset is above or below a predetermined level at expiration.
    BinaryOption = 12,
}

impl InstrumentClass {
    /// Returns whether this instrument class has an expiration.
    #[must_use]
    pub const fn has_expiration(&self) -> bool {
        matches!(
            self,
            Self::Future | Self::FuturesSpread | Self::Option | Self::OptionSpread
        )
    }

    /// Returns whether this instrument class allows negative prices.
    #[must_use]
    pub const fn allows_negative_price(&self) -> bool {
        matches!(
            self,
            Self::Option | Self::FuturesSpread | Self::OptionSpread
        )
    }

    /// Returns the [`InstrumentClass`] for the parent-symbol suffix, if recognised.
    ///
    /// Matches strict uppercase forms only. Both Databento-style abbreviations
    /// (`FUT`, `OPT`) and long forms (`FUTURE`, `OPTION`) are accepted.
    #[must_use]
    pub fn try_from_parent_suffix(suffix: &str) -> Option<Self> {
        match suffix {
            "FUT" | "FUTURE" => Some(Self::Future),
            "OPT" | "OPTION" => Some(Self::Option),
            _ => None,
        }
    }

    /// Returns the canonical parent-symbol suffix for this class, if one exists.
    ///
    /// Always emits the short form (`FUT`, `OPT`) so that adapters constructing
    /// parent ids produce a single canonical string per class.
    #[must_use]
    pub const fn parent_suffix(self) -> Option<&'static str> {
        match self {
            Self::Future => Some("FUT"),
            Self::Option => Some("OPT"),
            _ => None,
        }
    }
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum MarketStatus {
    /// The instrument is trading.
    Open = 1,
    /// Trading in the instrument has closed.
    Closed = 2,
    /// Trading in the instrument has been paused.
    Paused = 3,
    /// Trading in the instrument has been halted.
    Halted = 4,
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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

/// Convert the given `value` to a [`MarketStatusAction`].
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum OptionKind {
    /// A Call option gives the holder the right, but not the obligation, to buy an underlying asset at a specified strike price within a specified period of time.
    Call = 1,
    /// A Put option gives the holder the right, but not the obligation, to sell an underlying asset at a specified strike price within a specified period of time.
    Put = 2,
}

/// The numeraire convention for option greeks published by a venue.
///
/// Crypto option venues commonly publish two parallel greek sets for the same
/// instrument: Black-Scholes greeks in USD, and price-adjusted greeks denominated
/// in the underlying/coin units. Deribit and OKX both expose the distinction;
/// see the OKX reference for the canonical definition:
/// <https://www.okx.com/docs-v5/en/#public-data-websocket-option-market-data>.
///
/// This is orthogonal to the percent-greeks transformation in the internal
/// [`GreeksCalculator`](../../../nautilus_common/greeks/struct.GreeksCalculator.html),
/// which rescales the delta/gamma input step rather than the numeraire.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum GreeksConvention {
    /// Black-Scholes greeks in USD.
    #[default]
    BlackScholes = 1,
    /// Price-adjusted greeks in the underlying/coin units.
    PriceAdjusted = 2,
}

/// Defines when OTO (One-Triggers-Other) child orders are released.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum OtoTriggerMode {
    /// Release child order(s) pro-rata to each partial fill (default).
    #[default]
    Partial = 0,
    /// Release child order(s) only once the parent is fully filled.
    Full = 1,
}

/// The order side (BUY or SELL).
///
/// Python retains `NO_ORDER_SIDE` as a compatibility alias for `None`. The alias is not an enum
/// variant and may be removed in a future version.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum OrderSide {
    /// The order is a BUY.
    Buy = 1,
    /// The order is a SELL.
    Sell = 2,
}

impl OrderSide {
    /// Returns the opposite order side.
    #[must_use]
    pub fn opposite(&self) -> Self {
        match &self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
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
///  - `VOIDED`
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum OrderStatus {
    /// The order is initialized (instantiated) within the Nautilus system.
    Initialized = 1,
    /// The order was denied by the Nautilus system, either for being invalid, unprocessable, or exceeding a risk limit.
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
    /// The order is terminal after an authoritative venue void or fill correction.
    Voided = 15,
}

impl OrderStatus {
    /// Returns whether the order status represents an open/working order.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(
            self,
            Self::Submitted
                | Self::Accepted
                | Self::Triggered
                | Self::PendingUpdate
                | Self::PendingCancel
                | Self::PartiallyFilled
        )
    }

    /// Returns whether the order status represents a terminal (closed) state.
    #[must_use]
    pub const fn is_closed(self) -> bool {
        matches!(
            self,
            Self::Denied
                | Self::Rejected
                | Self::Canceled
                | Self::Expired
                | Self::Filled
                | Self::Voided
        )
    }

    /// Returns whether the order can be cancelled from this status.
    #[must_use]
    pub const fn is_cancellable(self) -> bool {
        matches!(
            self,
            Self::Accepted | Self::Triggered | Self::PendingUpdate | Self::PartiallyFilled
        )
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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

/// The type of position adjustment.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum PositionAdjustmentType {
    /// Commission adjustment affecting position quantity.
    Commission = 1,
    /// Funding payment affecting position realized PnL.
    Funding = 2,
}

impl FromU8 for PositionAdjustmentType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Commission),
            2 => Some(Self::Funding),
            _ => None,
        }
    }
}

/// The position side (FLAT, LONG, or SHORT).
///
/// Python retains `NO_POSITION_SIDE` as a compatibility alias for `None`. The alias is not an enum
/// variant and may be removed in a future version.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum PositionSide {
    /// A neutral/flat position, where no position is currently held in the market.
    Flat = 1,
    /// A long position in the market, typically acquired through one or many BUY orders.
    Long = 2,
    /// A short position in the market, typically acquired through one or many SELL orders.
    Short = 3,
}

/// Serde compatibility for an optional order side previously encoded with `NO_ORDER_SIDE`.
pub mod serde_option_order_side {
    use serde::{Deserializer, Serializer};

    use super::{OrderSide, deserialize_optional_enum, serialize_optional_enum};

    /// Serializes an optional order side using the legacy no-side token.
    ///
    /// # Errors
    ///
    /// Returns an error if the serializer cannot encode the value.
    pub fn serialize<S>(value: &Option<OrderSide>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_optional_enum(value.as_ref(), serializer, "NO_ORDER_SIDE")
    }

    /// Deserializes an optional order side from a side token or null.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is not a valid order side.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<OrderSide>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_optional_enum(
            deserializer,
            "NO_ORDER_SIDE",
            "BUY, SELL, NO_ORDER_SIDE, or null",
        )
    }
}

/// Serde compatibility for an optional position side previously encoded with `NO_POSITION_SIDE`.
pub mod serde_option_position_side {
    use serde::{Deserializer, Serializer};

    use super::{PositionSide, deserialize_optional_enum, serialize_optional_enum};

    /// Serializes an optional position side using the legacy no-side token.
    ///
    /// # Errors
    ///
    /// Returns an error if the serializer cannot encode the value.
    pub fn serialize<S>(value: &Option<PositionSide>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_optional_enum(value.as_ref(), serializer, "NO_POSITION_SIDE")
    }

    /// Deserializes an optional position side from a side token or null.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is not a valid position side.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<PositionSide>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_optional_enum(
            deserializer,
            "NO_POSITION_SIDE",
            "FLAT, LONG, SHORT, NO_POSITION_SIDE, or null",
        )
    }
}

/// Serde compatibility for an optional contingency type previously encoded with `NO_CONTINGENCY`.
pub mod serde_option_contingency_type {
    use serde::{Deserializer, Serializer};

    use super::{ContingencyType, deserialize_optional_enum, serialize_optional_enum};

    /// Serializes an optional contingency type using the legacy no-contingency token.
    ///
    /// # Errors
    ///
    /// Returns an error if the serializer cannot encode the value.
    pub fn serialize<S>(value: &Option<ContingencyType>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_optional_enum(value.as_ref(), serializer, "NO_CONTINGENCY")
    }

    /// Deserializes an optional contingency type from a contingency token or null.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is not a valid contingency type.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<ContingencyType>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_optional_enum(
            deserializer,
            "NO_CONTINGENCY",
            "OCO, OTO, OUO, NO_CONTINGENCY, or null",
        )
    }
}

/// Serde compatibility for an optional trailing offset type previously encoded with
/// `NO_TRAILING_OFFSET`.
pub mod serde_option_trailing_offset_type {
    use serde::{Deserializer, Serializer};

    use super::{TrailingOffsetType, deserialize_optional_enum, serialize_optional_enum};

    /// Serializes an optional trailing offset type using the legacy no-offset token.
    ///
    /// # Errors
    ///
    /// Returns an error if the serializer cannot encode the value.
    pub fn serialize<S>(
        value: &Option<TrailingOffsetType>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_optional_enum(value.as_ref(), serializer, "NO_TRAILING_OFFSET")
    }

    /// Deserializes an optional trailing offset type from an offset token or null.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is not a valid trailing offset type.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<TrailingOffsetType>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_optional_enum(
            deserializer,
            "NO_TRAILING_OFFSET",
            "PRICE, BASIS_POINTS, TICKS, PRICE_TIER, NO_TRAILING_OFFSET, or null",
        )
    }
}

/// Serde compatibility for an optional trigger type previously encoded with `NO_TRIGGER`.
pub mod serde_option_trigger_type {
    use serde::{Deserializer, Serializer};

    use super::{TriggerType, deserialize_optional_enum, serialize_optional_enum};

    /// Serializes an optional trigger type using the legacy no-trigger token.
    ///
    /// # Errors
    ///
    /// Returns an error if the serializer cannot encode the value.
    pub fn serialize<S>(value: &Option<TriggerType>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_optional_enum(value.as_ref(), serializer, "NO_TRIGGER")
    }

    /// Deserializes an optional trigger type from a trigger token or null.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is not a valid trigger type.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<TriggerType>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_optional_enum(
            deserializer,
            "NO_TRIGGER",
            "a trigger type, NO_TRIGGER, or null",
        )
    }
}

fn serialize_optional_enum<S, T>(
    value: Option<&T>,
    serializer: S,
    none_token: &'static str,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: AsRef<str>,
{
    serializer.serialize_str(value.map_or(none_token, AsRef::as_ref))
}

fn deserialize_optional_enum<'de, D, T>(
    deserializer: D,
    none_token: &'static str,
    expected: &'static str,
) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    struct OptionalEnumVisitor<T> {
        none_token: &'static str,
        expected: &'static str,
        marker: PhantomData<T>,
    }

    impl<'de, T> serde::de::Visitor<'de> for OptionalEnumVisitor<T>
    where
        T: FromStr,
        T::Err: Display,
    {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.expected)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            let value = Cow::<'de, str>::deserialize(deserializer)?;
            if value.eq_ignore_ascii_case(self.none_token) {
                Ok(None)
            } else {
                T::from_str(&value)
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
    }

    deserializer.deserialize_option(OptionalEnumVisitor {
        none_token,
        expected,
        marker: PhantomData,
    })
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum PriceType {
    // Bar price sources are not yet consistent with mark/index price subscriptions. The open
    // decisions are whether to add a `PriceType::Index` variant, whether to aggregate bars
    // internally from mark/index updates, and what the documented source derivation order is.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum TradingState {
    /// Normal trading operations.
    Active = 1,
    /// Trading is halted except for verified emergency position exits routed through an
    /// execution client which enforces reduce-only for the order.
    Halted = 2,
    /// Only order commands which would cancel order, or reduce position sizes are permitted.
    Reducing = 3,
}

/// The trailing offset type for an order type which specifies a trailing stop/trigger or limit price.
///
/// Python retains `NO_TRAILING_OFFSET` as a compatibility alias for `None`. The alias is not an enum
/// variant and may be removed in a future version.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum TrailingOffsetType {
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
///
/// Python retains `NO_TRIGGER` as a compatibility alias for `None`. The alias is not an enum variant
/// and may be removed in a future version.
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
        module = "nautilus_trader.model",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.model")
)]
pub enum TriggerType {
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
enum_strum_serde!(BarAggregation);
enum_strum_serde!(BarIntervalType);
enum_strum_serde!(BookAction);
enum_strum_serde!(BookType);
enum_strum_serde!(ContingencyType);
enum_strum_serde!(ContinuousFutureAdjustmentType);
enum_strum_serde!(CurrencyType);
enum_strum_serde!(GreeksConvention);
enum_strum_serde!(InstrumentClass);
enum_strum_serde!(InstrumentCloseType);
enum_strum_serde!(LiquiditySide);
enum_strum_serde!(MarketStatus);
enum_strum_serde!(MarketStatusAction);
enum_strum_serde!(OmsType);
enum_strum_serde!(OptionKind);
enum_strum_serde!(OrderSide);
enum_strum_serde!(OrderStatus);
enum_strum_serde!(OrderType);
enum_strum_serde!(PositionAdjustmentType);
enum_strum_serde!(PositionSide);
enum_strum_serde!(PriceType);
enum_strum_serde!(RecordFlag);
enum_strum_serde!(TimeInForce);
enum_strum_serde!(TradingState);
enum_strum_serde!(TrailingOffsetType);
enum_strum_serde!(TriggerType);

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct OptionalSides {
        #[serde(with = "serde_option_order_side")]
        order: Option<OrderSide>,
        #[serde(with = "serde_option_position_side")]
        position: Option<PositionSide>,
    }

    #[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct OptionalOrderTypes {
        #[serde(with = "serde_option_contingency_type")]
        contingency: Option<ContingencyType>,
        #[serde(with = "serde_option_trailing_offset_type")]
        trailing_offset: Option<TrailingOffsetType>,
        #[serde(with = "serde_option_trigger_type")]
        trigger: Option<TriggerType>,
    }

    #[rstest]
    fn test_optional_sides_serde_preserves_legacy_none_tokens() {
        let value = OptionalSides {
            order: None,
            position: None,
        };

        let json = serde_json::to_string(&value).unwrap();
        let decoded: OptionalSides = serde_json::from_str(&json).unwrap();

        assert_eq!(
            json,
            r#"{"order":"NO_ORDER_SIDE","position":"NO_POSITION_SIDE"}"#
        );
        assert_eq!(decoded, value);
    }

    #[rstest]
    fn test_optional_sides_serde_accepts_null_and_valid_sides() {
        let json = r#"{"order":null,"position":"LONG"}"#;
        let decoded: OptionalSides = serde_json::from_str(json).unwrap();

        assert_eq!(
            decoded,
            OptionalSides {
                order: None,
                position: Some(PositionSide::Long),
            }
        );
    }

    #[rstest]
    fn test_optional_order_types_serde_preserves_legacy_none_tokens() {
        let value = OptionalOrderTypes {
            contingency: None,
            trailing_offset: None,
            trigger: None,
        };

        let json = serde_json::to_string(&value).unwrap();
        let decoded: OptionalOrderTypes = serde_json::from_str(&json).unwrap();

        assert_eq!(
            json,
            r#"{"contingency":"NO_CONTINGENCY","trailing_offset":"NO_TRAILING_OFFSET","trigger":"NO_TRIGGER"}"#,
        );
        assert_eq!(decoded, value);
    }

    #[rstest]
    fn test_optional_order_types_serde_accepts_null_and_valid_values() {
        let json = r#"{"contingency":null,"trailing_offset":"PRICE","trigger":"LAST_PRICE"}"#;
        let decoded: OptionalOrderTypes = serde_json::from_str(json).unwrap();

        assert_eq!(
            decoded,
            OptionalOrderTypes {
                contingency: None,
                trailing_offset: Some(TrailingOffsetType::Price),
                trigger: Some(TriggerType::LastPrice),
            },
        );
    }

    #[rstest]
    #[case(r#"{"contingency":"INVALID","trailing_offset":"NO_TRAILING_OFFSET","trigger":"NO_TRIGGER"}"#)]
    #[case(
        r#"{"contingency":"NO_CONTINGENCY","trailing_offset":"INVALID","trigger":"NO_TRIGGER"}"#
    )]
    #[case(r#"{"contingency":"NO_CONTINGENCY","trailing_offset":"NO_TRAILING_OFFSET","trigger":"INVALID"}"#)]
    fn test_optional_order_types_serde_rejects_invalid_values(#[case] json: &str) {
        assert!(serde_json::from_str::<OptionalOrderTypes>(json).is_err());
    }

    #[rstest]
    #[case::no_aggressor(0, Some(AggressorSide::NoAggressor))]
    #[case::buy(1, Some(AggressorSide::Buy))]
    #[case::sell(2, Some(AggressorSide::Sell))]
    #[case::invalid(3, None)]
    #[case::max_u8(255, None)]
    fn test_aggressor_side_from_u8(#[case] value: u8, #[case] expected: Option<AggressorSide>) {
        assert_eq!(AggressorSide::from_u8(value), expected);
    }

    #[rstest]
    #[case(AggressorSide::NoAggressor, "NO_AGGRESSOR")]
    #[case(AggressorSide::Buy, "BUY")]
    #[case(AggressorSide::Sell, "SELL")]
    fn test_aggressor_side_to_string(#[case] value: AggressorSide, #[case] expected: &str) {
        assert_eq!(value.to_string(), expected);
        assert_eq!(value.as_ref(), expected);
    }

    #[rstest]
    #[case(AggressorSide::NoAggressor, "NO_AGGRESSOR")]
    #[case(AggressorSide::Buy, "BUY")]
    #[case(AggressorSide::Sell, "SELL")]
    #[case(AggressorSide::Buy, "BUYER")]
    #[case(AggressorSide::Sell, "SELLER")]
    #[case(AggressorSide::Buy, "buy")]
    #[case(AggressorSide::Sell, "seller")]
    fn test_aggressor_side_from_str(#[case] expected: AggressorSide, #[case] value: &str) {
        assert_eq!(AggressorSide::from_str(value), Ok(expected));
    }

    #[rstest]
    #[case(AggressorSide::Buy, "\"BUY\"")]
    #[case(AggressorSide::Sell, "\"SELL\"")]
    #[case(AggressorSide::NoAggressor, "\"NO_AGGRESSOR\"")]
    fn test_aggressor_side_serde_roundtrip(#[case] input: AggressorSide, #[case] expected: &str) {
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, expected);
        let parsed: AggressorSide = serde_json::from_str(expected).unwrap();
        assert_eq!(parsed, input);
    }

    #[rstest]
    #[case("BUYER", AggressorSide::Buy)]
    #[case("SELLER", AggressorSide::Sell)]
    fn test_aggressor_side_serde_accepts_historical(
        #[case] value: &str,
        #[case] expected: AggressorSide,
    ) {
        let parsed: AggressorSide = serde_json::from_str(&format!("\"{value}\"")).unwrap();
        assert_eq!(parsed, expected);
    }

    #[rstest]
    #[case(GreeksConvention::BlackScholes, "\"BLACK_SCHOLES\"")]
    #[case(GreeksConvention::PriceAdjusted, "\"PRICE_ADJUSTED\"")]
    fn test_greeks_convention_serde_roundtrip(
        #[case] input: GreeksConvention,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, expected);
        let parsed: GreeksConvention = serde_json::from_str(expected).unwrap();
        assert_eq!(parsed, input);
    }

    #[rstest]
    fn test_greeks_convention_default_is_black_scholes() {
        assert_eq!(GreeksConvention::default(), GreeksConvention::BlackScholes);
    }

    #[rstest]
    #[case(ContinuousFutureAdjustmentType::BackwardSpread, false, true)]
    #[case(ContinuousFutureAdjustmentType::ForwardSpread, false, false)]
    #[case(ContinuousFutureAdjustmentType::BackwardRatio, true, true)]
    #[case(ContinuousFutureAdjustmentType::ForwardRatio, true, false)]
    fn test_continuous_future_adjustment_type_predicates(
        #[case] mode: ContinuousFutureAdjustmentType,
        #[case] expected_is_ratio: bool,
        #[case] expected_is_backward: bool,
    ) {
        assert_eq!(mode.is_ratio(), expected_is_ratio);
        assert_eq!(mode.is_backward(), expected_is_backward);
    }

    #[rstest]
    #[case(ContinuousFutureAdjustmentType::BackwardSpread, "\"BACKWARD_SPREAD\"")]
    #[case(ContinuousFutureAdjustmentType::ForwardSpread, "\"FORWARD_SPREAD\"")]
    #[case(ContinuousFutureAdjustmentType::BackwardRatio, "\"BACKWARD_RATIO\"")]
    #[case(ContinuousFutureAdjustmentType::ForwardRatio, "\"FORWARD_RATIO\"")]
    fn test_continuous_future_adjustment_type_serde_roundtrip(
        #[case] input: ContinuousFutureAdjustmentType,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, expected);
        let parsed: ContinuousFutureAdjustmentType = serde_json::from_str(expected).unwrap();
        assert_eq!(parsed, input);
    }

    #[rstest]
    fn test_continuous_future_adjustment_type_default_is_backward_spread() {
        assert_eq!(
            ContinuousFutureAdjustmentType::default(),
            ContinuousFutureAdjustmentType::BackwardSpread,
        );
    }

    #[rstest]
    #[case(InstrumentClass::Option, true)]
    #[case(InstrumentClass::FuturesSpread, true)]
    #[case(InstrumentClass::OptionSpread, true)]
    #[case(InstrumentClass::Spot, false)]
    #[case(InstrumentClass::Swap, false)]
    #[case(InstrumentClass::Future, false)]
    #[case(InstrumentClass::Forward, false)]
    #[case(InstrumentClass::Cfd, false)]
    #[case(InstrumentClass::Bond, false)]
    #[case(InstrumentClass::Warrant, false)]
    #[case(InstrumentClass::SportsBetting, false)]
    #[case(InstrumentClass::BinaryOption, false)]
    fn test_instrument_class_allows_negative_price(
        #[case] class: InstrumentClass,
        #[case] expected: bool,
    ) {
        assert_eq!(class.allows_negative_price(), expected);
    }

    #[rstest]
    #[case("FUT", Some(InstrumentClass::Future))]
    #[case("FUTURE", Some(InstrumentClass::Future))]
    #[case("OPT", Some(InstrumentClass::Option))]
    #[case("OPTION", Some(InstrumentClass::Option))]
    #[case("fut", None)]
    #[case("Fut", None)]
    #[case("option", None)]
    #[case("Option", None)]
    #[case("SPREAD", None)]
    #[case("UNKNOWN", None)]
    #[case("", None)]
    fn test_instrument_class_try_from_parent_suffix(
        #[case] suffix: &str,
        #[case] expected: Option<InstrumentClass>,
    ) {
        assert_eq!(InstrumentClass::try_from_parent_suffix(suffix), expected);
    }

    #[rstest]
    #[case(InstrumentClass::Future, Some("FUT"))]
    #[case(InstrumentClass::Option, Some("OPT"))]
    #[case(InstrumentClass::Spot, None)]
    #[case(InstrumentClass::Swap, None)]
    #[case(InstrumentClass::FuturesSpread, None)]
    #[case(InstrumentClass::Forward, None)]
    #[case(InstrumentClass::Cfd, None)]
    #[case(InstrumentClass::Bond, None)]
    #[case(InstrumentClass::OptionSpread, None)]
    #[case(InstrumentClass::Warrant, None)]
    #[case(InstrumentClass::SportsBetting, None)]
    #[case(InstrumentClass::BinaryOption, None)]
    fn test_instrument_class_parent_suffix(
        #[case] class: InstrumentClass,
        #[case] expected: Option<&'static str>,
    ) {
        assert_eq!(class.parent_suffix(), expected);
    }

    #[rstest]
    #[case(InstrumentClass::Future)]
    #[case(InstrumentClass::Option)]
    fn test_instrument_class_parent_suffix_roundtrip(#[case] class: InstrumentClass) {
        let suffix = class.parent_suffix().unwrap();
        assert_eq!(InstrumentClass::try_from_parent_suffix(suffix), Some(class));
    }
}
