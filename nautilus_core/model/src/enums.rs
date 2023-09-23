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
use pyo3::{exceptions::PyValueError, prelude::*, types::PyType, PyTypeInfo};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};

use crate::{enum_for_python, enum_strum_serde, python::EnumIterator};

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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum AccountType {
    /// An account with unleveraged cash assets only.
    #[pyo3(name = "CASH")]
    Cash = 1,
    /// An account which facilitates trading on margin, using account assets as collateral.
    #[pyo3(name = "MARGIN")]
    Margin = 2,
    /// An account specific to betting markets.
    #[pyo3(name = "BETTING")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum AggregationSource {
    /// The data is externally aggregated (outside the Nautilus system boundary).
    #[pyo3(name = "EXTERNAL")]
    External = 1,
    /// The data is internally aggregated (inside the Nautilus system boundary).
    #[pyo3(name = "INTERNAL")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum AggressorSide {
    /// There was no specific aggressor for the trade.
    NoAggressor = 0, // Will be replaced by `Option`
    /// The BUY order was the aggressor for the trade.
    #[pyo3(name = "BUYER")]
    Buyer = 1,
    /// The SELL order was the aggressor for the trade.
    #[pyo3(name = "SELLER")]
    Seller = 2,
}

impl FromU8 for AggressorSide {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(AggressorSide::NoAggressor),
            1 => Some(AggressorSide::Buyer),
            2 => Some(AggressorSide::Seller),
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
#[allow(non_camel_case_types)]
pub enum AssetClass {
    /// Foreign exchange (FOREX) assets.
    FX = 1,
    /// Equity / stock assets.
    #[pyo3(name = "EQUITY")]
    Equity = 2,
    /// Commodity assets.
    #[pyo3(name = "COMMODITY")]
    Commodity = 3,
    /// Metal commodity assets.
    #[pyo3(name = "METAL")]
    Metal = 4,
    /// Energy commodity assets.
    #[pyo3(name = "ENERGY")]
    Energy = 5,
    /// Fixed income bond assets.
    #[pyo3(name = "BOND")]
    Bond = 6,
    /// Index based assets.
    #[pyo3(name = "INDEX")]
    Index = 7,
    /// Cryptocurrency or crypto token assets.
    #[pyo3(name = "CRYPTO_CURRENCY")]
    Cryptocurrency = 8,
    /// Sports betting instruments.
    #[pyo3(name = "SPORTS_BETTING")]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum AssetType {
    /// A spot market asset type. The current market price of an asset that is bought or sold for immediate delivery and payment.
    #[pyo3(name = "SPOT")]
    Spot = 1,
    /// A swap asset type. A derivative contract through which two parties exchange the cash flows or liabilities from two different financial instruments.
    #[pyo3(name = "SWAP")]
    Swap = 2,
    /// A futures contract asset type. A legal agreement to buy or sell an asset at a predetermined price at a specified time in the future.
    #[pyo3(name = "FUTURE")]
    Future = 3,
    /// A forward derivative asset type. A customized contract between two parties to buy or sell an asset at a specified price on a future date.
    #[pyo3(name = "FORWARD")]
    Forward = 4,
    /// A contract-for-difference (CFD) asset type. A contract between an investor and a CFD broker to exchange the difference in the value of a financial product between the time the contract opens and closes.
    #[pyo3(name = "CFD")]
    Cfd = 5,
    /// An options contract asset type. A type of derivative that gives the holder the right, but not the obligation, to buy or sell an underlying asset at a predetermined price before or at a certain future date.
    #[pyo3(name = "OPTION")]
    Option = 6,
    /// A warrant asset type. A derivative that gives the holder the right, but not the obligation, to buy or sell a security—most commonly an equity—at a certain price before expiration.
    #[pyo3(name = "WARRANT")]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum BarAggregation {
    /// Based on a number of ticks.
    #[pyo3(name = "TICK")]
    Tick = 1,
    /// Based on the buy/sell imbalance of ticks.
    #[pyo3(name = "TICK_IMBALANCE")]
    TickImbalance = 2,
    /// Based on sequential buy/sell runs of ticks.
    #[pyo3(name = "TICK_RUNS")]
    TickRuns = 3,
    /// Based on trading volume.
    #[pyo3(name = "VOLUME")]
    Volume = 4,
    /// Based on the buy/sell imbalance of trading volume.
    #[pyo3(name = "VOLUME_IMBALANCE")]
    VolumeImbalance = 5,
    /// Based on sequential runs of buy/sell trading volume.
    #[pyo3(name = "VOLUME_RUNS")]
    VolumeRuns = 6,
    /// Based on the 'notional' value of the instrument.
    #[pyo3(name = "VALUE")]
    Value = 7,
    /// Based on the buy/sell imbalance of trading by 'notional' value.
    #[pyo3(name = "VALUE_IMBALANCE")]
    ValueImbalance = 8,
    /// Based on sequential buy/sell runs of trading by 'notional' value.
    #[pyo3(name = "VALUE_RUNS")]
    ValueRuns = 9,
    /// Based on time intervals with millisecond granularity.
    #[pyo3(name = "MILLISECOND")]
    Millisecond = 10,
    /// Based on time intervals with second granularity.
    #[pyo3(name = "SECOND")]
    Second = 11,
    /// Based on time intervals with minute granularity.
    #[pyo3(name = "MINUTE")]
    Minute = 12,
    /// Based on time intervals with hour granularity.
    #[pyo3(name = "HOUR")]
    Hour = 13,
    /// Based on time intervals with day granularity.
    #[pyo3(name = "DAY")]
    Day = 14,
    /// Based on time intervals with week granularity.
    #[pyo3(name = "WEEK")]
    Week = 15,
    /// Based on time intervals with month granularity.
    #[pyo3(name = "MONTH")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum BookAction {
    /// An order is added to the book.
    #[pyo3(name = "ADD")]
    Add = 1,
    /// An existing order in the book is updated/modified.
    #[pyo3(name = "UPDATE")]
    Update = 2,
    /// An existing order in the book is deleted/canceled.
    #[pyo3(name = "DELETE")]
    Delete = 3,
    /// The state of the order book is cleared.
    #[pyo3(name = "CLEAR")]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[allow(non_camel_case_types)]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum ContingencyType {
    /// Not a contingent order.
    NoContingency = 0, // Will be replaced by `Option`
    /// One-Cancels-the-Other.
    #[pyo3(name = "OCO")]
    Oco = 1,
    /// One-Triggers-the-Other.
    #[pyo3(name = "OTO")]
    Oto = 2,
    /// One-Updates-the-Other (by proportional quantity).
    #[pyo3(name = "OUO")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum CurrencyType {
    /// A type of cryptocurrency or crypto token.
    #[pyo3(name = "CRYPTO")]
    Crypto = 1,
    /// A type of currency issued by governments which is not backed by a commodity.
    #[pyo3(name = "FIAT")]
    Fiat = 2,
    /// A type of currency that is based on the value of an underlying commodity.
    #[pyo3(name = "COMMODITY_BACKED")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum InstrumentCloseType {
    /// When the market session ended.
    #[pyo3(name = "END_OF_SESSION")]
    EndOfSession = 1,
    /// When the instrument expiration was reached.
    #[pyo3(name = "CONTRACT_EXPIRED")]
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
#[allow(clippy::enum_variant_names)]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum LiquiditySide {
    /// No specific liqudity side.
    NoLiquiditySide = 0, // Will be replaced by `Option`
    /// The order passively provided liqudity to the market to complete the trade (made a market).
    #[pyo3(name = "MAKER")]
    Maker = 1,
    /// The order aggressively took liqudity from the market to complete the trade.
    #[pyo3(name = "TAKER")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum MarketStatus {
    /// The market is closed.
    #[pyo3(name = "CLOSED")]
    Closed = 1,
    /// The market is in the pre-open session.
    #[pyo3(name = "PRE_OPEN")]
    PreOpen = 2,
    /// The market is open for the normal session.
    #[pyo3(name = "OPEN")]
    Open = 3,
    /// The market session is paused.
    #[pyo3(name = "PAUSE")]
    Pause = 4,
    /// The market is in the pre-close session.
    #[pyo3(name = "PRE_CLOSE")]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum OmsType {
    /// There is no specific type of order management specified (will defer to the venue).
    Unspecified = 0, // Will be replaced by `Option`
    /// The netting type where there is one position per instrument.
    #[pyo3(name = "NETTING")]
    Netting = 1,
    /// The hedging type where there can be multiple positions per instrument.
    /// This can be in LONG/SHORT directions, by position/ticket ID, or tracked virtually by
    /// Nautilus.
    #[pyo3(name = "HEDGING")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum OptionKind {
    /// A Call option gives the holder the right, but not the obligation, to buy an underlying asset at a specified strike price within a specified period of time.
    #[pyo3(name = "CALL")]
    Call = 1,
    /// A Put option gives the holder the right, but not the obligation, to sell an underlying asset at a specified strike price within a specified period of time.
    #[pyo3(name = "PUT")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum OrderSide {
    /// No order side is specified (only valid in the context of a filter for actions involving orders).
    NoOrderSide = 0, // Will be replaced by `Option`
    /// The order is a BUY.
    #[pyo3(name = "BUY")]
    Buy = 1,
    /// The order is a SELL.
    #[pyo3(name = "SELL")]
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
    EnumIter,
    EnumString,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum OrderStatus {
    /// The order is initialized (instantiated) within the Nautilus system.
    #[pyo3(name = "INITIALIZED")]
    Initialized = 1,
    /// The order was denied by the Nautilus system, either for being invalid, unprocessable or exceeding a risk limit.
    #[pyo3(name = "DENIED")]
    Denied = 2,
    /// The order became emulated by the Nautilus system in the `OrderEmulator` component.
    #[pyo3(name = "EMULATED")]
    Emulated = 3,
    /// The order was released by the Nautilus system from the `OrderEmulator` component.
    #[pyo3(name = "RELEASED")]
    Released = 4,
    /// The order was submitted by the Nautilus system to the external service or trading venue (awaiting acknowledgement).
    #[pyo3(name = "SUBMITTED")]
    Submitted = 5,
    /// The order was acknowledged by the trading venue as being received and valid (may now be working).
    #[pyo3(name = "ACCEPTED")]
    Accepted = 6,
    /// The order was rejected by the trading venue.
    #[pyo3(name = "REJECTED")]
    Rejected = 7,
    /// The order was canceled (closed/done).
    #[pyo3(name = "CANCELED")]
    Canceled = 8,
    /// The order reached a GTD expiration (closed/done).
    #[pyo3(name = "EXPIRED")]
    Expired = 9,
    /// The order STOP price was triggered on a trading venue.
    #[pyo3(name = "TRIGGERED")]
    Triggered = 10,
    /// The order is currently pending a request to modify on a trading venue.
    #[pyo3(name = "PENDING_UPDATE")]
    PendingUpdate = 11,
    /// The order is currently pending a request to cancel on a trading venue.
    #[pyo3(name = "PENDING_CANCEL")]
    PendingCancel = 12,
    /// The order has been partially filled on a trading venue.
    #[pyo3(name = "PARTIALLY_FILLED")]
    PartiallyFilled = 13,
    /// The order has been completely filled on a trading venue (closed/done).
    #[pyo3(name = "FILLED")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum OrderType {
    /// A market order to buy or sell at the best available price in the current market.
    #[pyo3(name = "MARKET")]
    Market = 1,
    /// A limit order to buy or sell at a specific price or better.
    #[pyo3(name = "LIMIT")]
    Limit = 2,
    /// A stop market order to buy or sell once the price reaches the specified stop/trigger price. When the stop price is reached, the order effectively becomes a market order.
    #[pyo3(name = "STOP_MARKET")]
    StopMarket = 3,
    /// A stop limit order to buy or sell which combines the features of a stop order and a limit order. Once the stop/trigger price is reached, a stop-limit order effectively becomes a limit order.
    #[pyo3(name = "STOP_LIMIT")]
    StopLimit = 4,
    /// A market-to-limit order is a market order that is to be executed as a limit order at the current best market price after reaching the market.
    #[pyo3(name = "MARKET_TO_LIMIT")]
    MarketToLimit = 5,
    /// A market-if-touched order effectively becomes a market order when the specified trigger price is reached.
    #[pyo3(name = "MARKET_IF_TOUCHED")]
    MarketIfTouched = 6,
    /// A limit-if-touched order effectively becomes a limit order when the specified trigger price is reached.
    #[pyo3(name = "LIMIT_IF_TOUCHED")]
    LimitIfTouched = 7,
    /// A trailing stop market order sets the stop/trigger price at a fixed "trailing offset" amount from the market.
    #[pyo3(name = "TRAILING_STOP_MARKET")]
    TrailingStopMarket = 8,
    /// A trailing stop limit order combines the features of a trailing stop order with those of a limit order.
    #[pyo3(name = "TRAILING_STOP_LIMIT")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum PositionSide {
    /// No position side is specified (only valid in the context of a filter for actions involving positions).
    NoPositionSide = 0, // Will be replaced by `Option`
    /// A neural/flat position, where no position is currently held in the market.
    #[pyo3(name = "FLAT")]
    Flat = 1,
    /// A long position in the market, typically acquired through one or many BUY orders.
    #[pyo3(name = "LONG")]
    Long = 2,
    /// A short position in the market, typically acquired through one or many SELL orders.
    #[pyo3(name = "SHORT")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum PriceType {
    /// A quoted order price where a buyer is willing to buy a quantity of an instrument.
    #[pyo3(name = "BID")]
    Bid = 1,
    /// A quoted order price where a seller is willing to sell a quantity of an instrument.
    #[pyo3(name = "ASK")]
    Ask = 2,
    /// The midpoint between the bid and ask prices.
    #[pyo3(name = "MID")]
    Mid = 3,
    /// The last price at which a trade was made for an instrument.
    #[pyo3(name = "LAST")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum TimeInForce {
    /// Good Till Canceled (GTC) - the order remains active until canceled.
    #[pyo3(name = "GTD")]
    Gtc = 1,
    /// Immediate or Cancel (IOC) - the order is filled as much as possible, the rest is canceled.
    #[pyo3(name = "IOC")]
    Ioc = 2,
    /// Fill or Kill (FOK) - the order must be executed in full immediately, or it is canceled.
    #[pyo3(name = "FOK")]
    Fok = 3,
    /// Good Till Date/Time (GTD) - the order is active until a specified date or time.
    #[pyo3(name = "GTD")]
    Gtd = 4,
    /// Day - the order is active until the end of the current trading session.
    #[pyo3(name = "DAY")]
    Day = 5,
    /// At the Opening (ATO) - the order is scheduled to be executed at the market's opening.
    #[pyo3(name = "AT_THE_OPEN")]
    AtTheOpen = 6,
    /// At the Closing (ATC) - the order is scheduled to be executed at the market's closing.
    #[pyo3(name = "AT_THE_CLOSE")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum TradingState {
    /// Normal trading operations.
    #[pyo3(name = "ACTIVE")]
    Active = 1,
    /// Trading is completely halted, no new order commands will be emitted.
    #[pyo3(name = "HALTED")]
    Halted = 2,
    /// Only order commands which would cancel order, or reduce position sizes are permitted.
    #[pyo3(name = "REDUCING")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum TrailingOffsetType {
    /// No trailing offset type is specified (invalid for trailing type orders).
    NoTrailingOffset = 0, // Will be replaced by `Option`
    /// The trailing offset is based on a market price.
    #[pyo3(name = "PRICE")]
    Price = 1,
    /// The trailing offset is based on a percentage represented in basis points, of a market price.
    #[pyo3(name = "BASIS_POINTS")]
    BasisPoints = 2,
    /// The trailing offset is based on the number of ticks from a market price.
    #[pyo3(name = "TICKS")]
    Ticks = 3,
    /// The trailing offset is based on a price tier set by a specific trading venue.
    #[pyo3(name = "PRICE_TIER")]
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
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.enums")]
pub enum TriggerType {
    /// No trigger type is specified (invalid for orders with a trigger).
    NoTrigger = 0, // Will be replaced by `Option`
    /// The default trigger type set by the trading venue.
    #[pyo3(name = "DEFAULT")]
    Default = 1,
    /// Based on the top-of-book quoted prices for the instrument.
    #[pyo3(name = "BID_ASK")]
    BidAsk = 2,
    /// Based on the last traded price for the instrument.
    #[pyo3(name = "LAST_TRADE")]
    LastTrade = 3,
    /// Based on a 'double match' of the last traded price for the instrument
    #[pyo3(name = "DOUBLE_LAST")]
    DoubleLast = 4,
    /// Based on a 'double match' of the bid/ask price for the instrument
    #[pyo3(name = "DOUBLE_BID_ASK")]
    DoubleBidAsk = 5,
    /// Based on both the [`TriggerType::LastTrade`] and [`TriggerType::BidAsk`].
    #[pyo3(name = "LAST_OR_BID_ASK")]
    LastOrBidAsk = 6,
    /// Based on the mid-point of the [`TriggerType::BidAsk`].
    #[pyo3(name = "MID_POINT")]
    MidPoint = 7,
    /// Based on the mark price for the instrument.
    #[pyo3(name = "MARK_PRICE")]
    MarkPrice = 8,
    /// Based on the index price for the instrument.
    #[pyo3(name = "INDEX_PRICE")]
    IndexPrice = 9,
}

enum_strum_serde!(AccountType);
enum_strum_serde!(AggregationSource);
enum_strum_serde!(AggressorSide);
enum_strum_serde!(AssetClass);
enum_strum_serde!(AssetType);
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

enum_for_python!(AccountType);
enum_for_python!(AggregationSource);
enum_for_python!(AggressorSide);
enum_for_python!(AssetClass);
enum_for_python!(BarAggregation);
enum_for_python!(BookAction);
enum_for_python!(BookType);
enum_for_python!(ContingencyType);
enum_for_python!(CurrencyType);
enum_for_python!(InstrumentCloseType);
enum_for_python!(LiquiditySide);
enum_for_python!(MarketStatus);
enum_for_python!(OmsType);
enum_for_python!(OptionKind);
enum_for_python!(OrderSide);
enum_for_python!(OrderStatus);
enum_for_python!(OrderType);
enum_for_python!(PositionSide);
enum_for_python!(PriceType);
enum_for_python!(TimeInForce);
enum_for_python!(TradingState);
enum_for_python!(TrailingOffsetType);
enum_for_python!(TriggerType);

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn account_type_to_cstr(value: AccountType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn account_type_from_cstr(ptr: *const c_char) -> AccountType {
    let value = cstr_to_string(ptr);
    AccountType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AccountType` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn aggregation_source_to_cstr(value: AggregationSource) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn aggregation_source_from_cstr(ptr: *const c_char) -> AggregationSource {
    let value = cstr_to_string(ptr);
    AggregationSource::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AggregationSource` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn aggressor_side_to_cstr(value: AggressorSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn aggressor_side_from_cstr(ptr: *const c_char) -> AggressorSide {
    let value = cstr_to_string(ptr);
    AggressorSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AggressorSide` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn asset_class_to_cstr(value: AssetClass) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn asset_class_from_cstr(ptr: *const c_char) -> AssetClass {
    let value = cstr_to_string(ptr);
    AssetClass::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AssetClass` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn asset_type_to_cstr(value: AssetType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn asset_type_from_cstr(ptr: *const c_char) -> AssetType {
    let value = cstr_to_string(ptr);
    AssetType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `AssetType` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn bar_aggregation_to_cstr(value: BarAggregation) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn bar_aggregation_from_cstr(ptr: *const c_char) -> BarAggregation {
    let value = cstr_to_string(ptr);
    BarAggregation::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BarAggregation` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn book_action_to_cstr(value: BookAction) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn book_action_from_cstr(ptr: *const c_char) -> BookAction {
    let value = cstr_to_string(ptr);
    BookAction::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BookAction` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn book_type_to_cstr(value: BookType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn book_type_from_cstr(ptr: *const c_char) -> BookType {
    let value = cstr_to_string(ptr);
    BookType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `BookType` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn contingency_type_to_cstr(value: ContingencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn contingency_type_from_cstr(ptr: *const c_char) -> ContingencyType {
    let value = cstr_to_string(ptr);
    ContingencyType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `ContingencyType` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn currency_type_to_cstr(value: CurrencyType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
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
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn instrument_close_type_from_cstr(
    ptr: *const c_char,
) -> InstrumentCloseType {
    let value = cstr_to_string(ptr);
    InstrumentCloseType::from_str(&value).unwrap_or_else(|_| {
        panic!("invalid `InstrumentCloseType` enum string value, was '{value}'")
    })
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn instrument_close_type_to_cstr(value: InstrumentCloseType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn liquidity_side_to_cstr(value: LiquiditySide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn liquidity_side_from_cstr(ptr: *const c_char) -> LiquiditySide {
    let value = cstr_to_string(ptr);
    LiquiditySide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `LiquiditySide` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn market_status_to_cstr(value: MarketStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn market_status_from_cstr(ptr: *const c_char) -> MarketStatus {
    let value = cstr_to_string(ptr);
    MarketStatus::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `MarketStatus` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn oms_type_to_cstr(value: OmsType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn oms_type_from_cstr(ptr: *const c_char) -> OmsType {
    let value = cstr_to_string(ptr);
    OmsType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OmsType` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn option_kind_to_cstr(value: OptionKind) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn option_kind_from_cstr(ptr: *const c_char) -> OptionKind {
    let value = cstr_to_string(ptr);
    OptionKind::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OptionKind` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn order_side_to_cstr(value: OrderSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn order_side_from_cstr(ptr: *const c_char) -> OrderSide {
    let value = cstr_to_string(ptr);
    OrderSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderSide` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn order_status_to_cstr(value: OrderStatus) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn order_status_from_cstr(ptr: *const c_char) -> OrderStatus {
    let value = cstr_to_string(ptr);
    OrderStatus::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderStatus` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn order_type_to_cstr(value: OrderType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn order_type_from_cstr(ptr: *const c_char) -> OrderType {
    let value = cstr_to_string(ptr);
    OrderType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `OrderType` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn position_side_to_cstr(value: PositionSide) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn position_side_from_cstr(ptr: *const c_char) -> PositionSide {
    let value = cstr_to_string(ptr);
    PositionSide::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `PositionSide` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn price_type_to_cstr(value: PriceType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn price_type_from_cstr(ptr: *const c_char) -> PriceType {
    let value = cstr_to_string(ptr);
    PriceType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `PriceType` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn time_in_force_to_cstr(value: TimeInForce) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn time_in_force_from_cstr(ptr: *const c_char) -> TimeInForce {
    let value = cstr_to_string(ptr);
    TimeInForce::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TimeInForce` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn trading_state_to_cstr(value: TradingState) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn trading_state_from_cstr(ptr: *const c_char) -> TradingState {
    let value = cstr_to_string(ptr);
    TradingState::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TradingState` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn trailing_offset_type_to_cstr(value: TrailingOffsetType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn trailing_offset_type_from_cstr(ptr: *const c_char) -> TrailingOffsetType {
    let value = cstr_to_string(ptr);
    TrailingOffsetType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TrailingOffsetType` enum string value, was '{value}'"))
}

#[cfg(feature = "ffi")]
#[no_mangle]
pub extern "C" fn trigger_type_to_cstr(value: TriggerType) -> *const c_char {
    str_to_cstr(value.as_ref())
}

/// Returns an enum from a Python string.
///
/// # Safety
/// - Assumes `ptr` is a valid C string pointer.
#[cfg(feature = "ffi")]
#[no_mangle]
pub unsafe extern "C" fn trigger_type_from_cstr(ptr: *const c_char) -> TriggerType {
    let value = cstr_to_string(ptr);
    TriggerType::from_str(&value)
        .unwrap_or_else(|_| panic!("invalid `TriggerType` enum string value, was '{value}'"))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_name() {
        assert_eq!(OrderSide::NoOrderSide.name(), "NO_ORDER_SIDE");
        assert_eq!(OrderSide::Buy.name(), "BUY");
        assert_eq!(OrderSide::Sell.name(), "SELL");
    }

    #[rstest]
    fn test_value() {
        assert_eq!(OrderSide::NoOrderSide.value(), 0);
        assert_eq!(OrderSide::Buy.value(), 1);
        assert_eq!(OrderSide::Sell.value(), 2);
    }
}
