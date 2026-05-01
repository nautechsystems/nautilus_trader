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

//! Enumerations for the Interactive Brokers adapter.

use std::{fmt::Display, str::FromStr};

use nautilus_model::enums::{
    OptionKind, OrderSide, OrderStatus as NautilusOrderStatus, OrderType as NautilusOrderType,
    TimeInForce as NautilusTimeInForce,
};

/// Interactive Brokers execution/action side values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbAction {
    /// Buy action from order data.
    Buy,
    /// Bought side from execution data.
    Bought,
    /// Sell action from order data.
    Sell,
    /// Sold side from execution data.
    Sold,
    /// Sell short action from order data.
    SellShort,
    /// Sell long action from order data.
    SellLong,
}

impl IbAction {
    /// Converts the IB action to a Nautilus order side.
    #[must_use]
    pub const fn order_side(self) -> OrderSide {
        match self {
            Self::Buy | Self::Bought => OrderSide::Buy,
            Self::Sell | Self::Sold | Self::SellShort | Self::SellLong => OrderSide::Sell,
        }
    }

    /// Returns `1` for buy/bought and `-1` for sell/sold.
    #[must_use]
    pub const fn signed_multiplier(self) -> i32 {
        match self {
            Self::Buy | Self::Bought => 1,
            Self::Sell | Self::Sold | Self::SellShort | Self::SellLong => -1,
        }
    }

    /// Converts to the rust-ibapi order action enum.
    #[must_use]
    pub const fn ibapi_action(self) -> ibapi::orders::Action {
        match self {
            Self::Buy | Self::Bought => ibapi::orders::Action::Buy,
            Self::Sell | Self::Sold => ibapi::orders::Action::Sell,
            Self::SellShort => ibapi::orders::Action::SellShort,
            Self::SellLong => ibapi::orders::Action::SellLong,
        }
    }
}

impl FromStr for IbAction {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "BUY" => Ok(Self::Buy),
            "BOT" => Ok(Self::Bought),
            "SELL" => Ok(Self::Sell),
            "SLD" => Ok(Self::Sold),
            "SSHORT" => Ok(Self::SellShort),
            "SLONG" => Ok(Self::SellLong),
            _ => anyhow::bail!("Unknown IB action: {value}"),
        }
    }
}

impl From<ibapi::orders::Action> for IbAction {
    fn from(value: ibapi::orders::Action) -> Self {
        match value {
            ibapi::orders::Action::Buy => Self::Buy,
            ibapi::orders::Action::Sell => Self::Sell,
            ibapi::orders::Action::SellShort => Self::SellShort,
            ibapi::orders::Action::SellLong => Self::SellLong,
        }
    }
}

impl Display for IbAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Buy => "BUY",
            Self::Bought => "BOT",
            Self::Sell => "SELL",
            Self::Sold => "SLD",
            Self::SellShort => "SSHORT",
            Self::SellLong => "SLONG",
        })
    }
}

/// Interactive Brokers order status values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOrderStatus {
    /// Order is pending API processing.
    ApiPending,
    /// Order is pending submission.
    PendingSubmit,
    /// Order has been pre-submitted.
    PreSubmitted,
    /// Order has been submitted.
    Submitted,
    /// Order is pending cancellation.
    PendingCancel,
    /// Order was cancelled by the API.
    ApiCancelled,
    /// Order was cancelled.
    Cancelled,
    /// Order has been fully filled.
    Filled,
    /// Order is inactive.
    Inactive,
}

impl IbOrderStatus {
    /// Converts the IB order status to a Nautilus order status.
    #[must_use]
    pub const fn nautilus_status(self) -> NautilusOrderStatus {
        match self {
            Self::ApiPending | Self::PendingSubmit | Self::PreSubmitted => {
                NautilusOrderStatus::Submitted
            }
            Self::Submitted => NautilusOrderStatus::Accepted,
            Self::PendingCancel => NautilusOrderStatus::PendingCancel,
            Self::ApiCancelled | Self::Cancelled => NautilusOrderStatus::Canceled,
            Self::Filled => NautilusOrderStatus::Filled,
            Self::Inactive => NautilusOrderStatus::Rejected,
        }
    }

    /// Returns whether this status means an order should be treated as accepted.
    #[must_use]
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Submitted | Self::PreSubmitted)
    }

    /// Returns whether this status is terminal for pending spread fill handling.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Filled | Self::ApiCancelled | Self::Cancelled | Self::Inactive
        )
    }
}

impl FromStr for IbOrderStatus {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ApiPending" => Ok(Self::ApiPending),
            "PendingSubmit" => Ok(Self::PendingSubmit),
            "PreSubmitted" => Ok(Self::PreSubmitted),
            "Submitted" => Ok(Self::Submitted),
            "PendingCancel" => Ok(Self::PendingCancel),
            "ApiCancelled" => Ok(Self::ApiCancelled),
            "Cancelled" => Ok(Self::Cancelled),
            "Filled" => Ok(Self::Filled),
            "Inactive" => Ok(Self::Inactive),
            _ => anyhow::bail!("Unknown IB order status: {value}"),
        }
    }
}

impl Display for IbOrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ApiPending => "ApiPending",
            Self::PendingSubmit => "PendingSubmit",
            Self::PreSubmitted => "PreSubmitted",
            Self::Submitted => "Submitted",
            Self::PendingCancel => "PendingCancel",
            Self::ApiCancelled => "ApiCancelled",
            Self::Cancelled => "Cancelled",
            Self::Filled => "Filled",
            Self::Inactive => "Inactive",
        })
    }
}

/// Interactive Brokers order type values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOrderType {
    /// Market order.
    Market,
    /// Market-on-close order.
    MarketOnClose,
    /// Limit order.
    Limit,
    /// Limit-on-close order.
    LimitOnClose,
    /// Stop market order.
    Stop,
    /// Stop limit order.
    StopLimit,
    /// Trailing stop market order.
    TrailingStop,
    /// Trailing stop limit order.
    TrailingStopLimit,
    /// Market-if-touched order.
    MarketIfTouched,
    /// Limit-if-touched order.
    LimitIfTouched,
    /// Market-to-limit order.
    MarketToLimit,
    /// Market-on-open order.
    MarketOnOpen,
    /// Limit-on-open order.
    LimitOnOpen,
    /// Auction order routed to an exchange auction.
    AtAuction,
    /// Market order with price protection.
    MarketWithProtection,
    /// Stop order with price protection.
    StopWithProtection,
    /// Midprice order targeting the NBBO midpoint.
    Midprice,
    /// Pegged-to-market order.
    PeggedToMarket,
    /// Pegged-to-stock order.
    PeggedToStock,
    /// Pegged-to-midpoint order.
    PeggedToMidpoint,
    /// Pegged-to-benchmark order.
    PeggedToBenchmark,
    /// Peg-to-best order.
    PegBest,
    /// Relative order.
    Relative,
    /// Passive relative order.
    PassiveRelative,
    /// Volatility order.
    Volatility,
    /// Box-top order.
    BoxTop,
    /// Auction limit order.
    AuctionLimit,
    /// Auction relative order.
    AuctionRelative,
    /// Combo limit order.
    ComboLimit,
    /// Combo market order.
    ComboMarket,
    /// Relative plus limit combo order.
    RelativeLimitCombo,
    /// Relative plus market combo order.
    RelativeMarketCombo,
}

impl IbOrderType {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Market => "MKT",
            Self::MarketOnClose => "MOC",
            Self::Limit => "LMT",
            Self::LimitOnClose => "LOC",
            Self::Stop => "STP",
            Self::StopLimit => "STP LMT",
            Self::TrailingStop => "TRAIL",
            Self::TrailingStopLimit => "TRAIL LIMIT",
            Self::MarketIfTouched => "MIT",
            Self::LimitIfTouched => "LIT",
            Self::MarketToLimit => "MTL",
            Self::MarketOnOpen => "MKT",
            Self::LimitOnOpen => "LMT",
            Self::AtAuction => "MTL",
            Self::MarketWithProtection => "MKT PRT",
            Self::StopWithProtection => "STP PRT",
            Self::Midprice => "MIDPRICE",
            Self::PeggedToMarket => "PEG MKT",
            Self::PeggedToStock => "PEG STK",
            Self::PeggedToMidpoint => "PEG MID",
            Self::PeggedToBenchmark => "PEG BENCH",
            Self::PegBest => "PEG BEST",
            Self::Relative => "REL",
            Self::PassiveRelative => "PASSV REL",
            Self::Volatility => "VOL",
            Self::BoxTop => "BOX TOP",
            Self::AuctionLimit => "LMT",
            Self::AuctionRelative => "REL",
            Self::ComboLimit => "LMT",
            Self::ComboMarket => "MKT",
            Self::RelativeLimitCombo => "REL + LMT",
            Self::RelativeMarketCombo => "REL + MKT",
        }
    }

    /// Converts the IB order type to a Nautilus order type.
    #[must_use]
    pub const fn nautilus_order_type(self) -> NautilusOrderType {
        match self {
            Self::Market | Self::MarketOnClose => NautilusOrderType::Market,
            Self::Limit | Self::LimitOnClose => NautilusOrderType::Limit,
            Self::Stop => NautilusOrderType::StopMarket,
            Self::StopLimit => NautilusOrderType::StopLimit,
            Self::TrailingStop => NautilusOrderType::TrailingStopMarket,
            Self::TrailingStopLimit => NautilusOrderType::TrailingStopLimit,
            Self::MarketIfTouched => NautilusOrderType::MarketIfTouched,
            Self::LimitIfTouched => NautilusOrderType::LimitIfTouched,
            Self::MarketToLimit => NautilusOrderType::MarketToLimit,
            Self::MarketOnOpen
            | Self::AtAuction
            | Self::MarketWithProtection
            | Self::Midprice
            | Self::PeggedToMarket
            | Self::PeggedToStock
            | Self::PeggedToMidpoint
            | Self::PeggedToBenchmark
            | Self::PegBest
            | Self::Relative
            | Self::PassiveRelative
            | Self::Volatility
            | Self::BoxTop
            | Self::ComboMarket
            | Self::RelativeMarketCombo => NautilusOrderType::Market,
            Self::LimitOnOpen
            | Self::AuctionLimit
            | Self::ComboLimit
            | Self::RelativeLimitCombo => NautilusOrderType::Limit,
            Self::StopWithProtection | Self::AuctionRelative => NautilusOrderType::StopMarket,
        }
    }

    /// Converts a Nautilus order type and time-in-force pair to the IB order type.
    #[must_use]
    pub const fn from_nautilus(
        order_type: NautilusOrderType,
        time_in_force: NautilusTimeInForce,
    ) -> Self {
        match order_type {
            NautilusOrderType::Market => {
                if matches!(time_in_force, NautilusTimeInForce::AtTheClose) {
                    Self::MarketOnClose
                } else {
                    Self::Market
                }
            }
            NautilusOrderType::Limit => {
                if matches!(time_in_force, NautilusTimeInForce::AtTheClose) {
                    Self::LimitOnClose
                } else {
                    Self::Limit
                }
            }
            NautilusOrderType::StopMarket => Self::Stop,
            NautilusOrderType::StopLimit => Self::StopLimit,
            NautilusOrderType::MarketIfTouched => Self::MarketIfTouched,
            NautilusOrderType::LimitIfTouched => Self::LimitIfTouched,
            NautilusOrderType::TrailingStopMarket => Self::TrailingStop,
            NautilusOrderType::TrailingStopLimit => Self::TrailingStopLimit,
            NautilusOrderType::MarketToLimit => Self::MarketToLimit,
        }
    }
}

impl FromStr for IbOrderType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "MKT" => Ok(Self::Market),
            "MOC" => Ok(Self::MarketOnClose),
            "LMT" => Ok(Self::Limit),
            "LOC" => Ok(Self::LimitOnClose),
            "STP" => Ok(Self::Stop),
            "STP LMT" => Ok(Self::StopLimit),
            "TRAIL" => Ok(Self::TrailingStop),
            "TRAIL LIMIT" => Ok(Self::TrailingStopLimit),
            "MIT" => Ok(Self::MarketIfTouched),
            "LIT" => Ok(Self::LimitIfTouched),
            "MTL" => Ok(Self::MarketToLimit),
            "MKT PRT" => Ok(Self::MarketWithProtection),
            "STP PRT" => Ok(Self::StopWithProtection),
            "MIDPRICE" => Ok(Self::Midprice),
            "PEG MKT" => Ok(Self::PeggedToMarket),
            "PEG STK" => Ok(Self::PeggedToStock),
            "PEG MID" => Ok(Self::PeggedToMidpoint),
            "PEG BENCH" => Ok(Self::PeggedToBenchmark),
            "PEG BEST" => Ok(Self::PegBest),
            "REL" => Ok(Self::Relative),
            "PASSV REL" => Ok(Self::PassiveRelative),
            "VOL" => Ok(Self::Volatility),
            "BOX TOP" => Ok(Self::BoxTop),
            "REL + LMT" => Ok(Self::RelativeLimitCombo),
            "REL + MKT" => Ok(Self::RelativeMarketCombo),
            _ => anyhow::bail!("Unknown IB order type: {value}"),
        }
    }
}

impl Display for IbOrderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers time-in-force values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTimeInForce {
    /// Day order.
    Day,
    /// Good-till-cancelled order.
    GoodTilCanceled,
    /// Immediate-or-cancel order.
    ImmediateOrCancel,
    /// Good-till-date order.
    GoodTilDate,
    /// Opening auction order.
    OnOpen,
    /// Fill-or-kill order.
    FillOrKill,
    /// Day-till-cancelled order.
    DayTilCanceled,
    /// Auction order.
    Auction,
}

impl IbTimeInForce {
    /// Converts this IB time-in-force to a Nautilus time-in-force.
    #[must_use]
    pub const fn nautilus_time_in_force(self) -> NautilusTimeInForce {
        match self {
            Self::Day | Self::DayTilCanceled | Self::Auction => NautilusTimeInForce::Day,
            Self::GoodTilCanceled => NautilusTimeInForce::Gtc,
            Self::ImmediateOrCancel => NautilusTimeInForce::Ioc,
            Self::GoodTilDate => NautilusTimeInForce::Gtd,
            Self::OnOpen => NautilusTimeInForce::AtTheOpen,
            Self::FillOrKill => NautilusTimeInForce::Fok,
        }
    }

    /// Converts a Nautilus time-in-force to the corresponding IB time-in-force.
    #[must_use]
    pub const fn from_nautilus(time_in_force: NautilusTimeInForce) -> Self {
        match time_in_force {
            NautilusTimeInForce::Day | NautilusTimeInForce::AtTheClose => Self::Day,
            NautilusTimeInForce::Gtc => Self::GoodTilCanceled,
            NautilusTimeInForce::Ioc => Self::ImmediateOrCancel,
            NautilusTimeInForce::Fok => Self::FillOrKill,
            NautilusTimeInForce::Gtd => Self::GoodTilDate,
            NautilusTimeInForce::AtTheOpen => Self::OnOpen,
        }
    }

    /// Converts this adapter enum to the rust-ibapi enum.
    #[must_use]
    pub const fn ibapi_time_in_force(self) -> ibapi::orders::TimeInForce {
        match self {
            Self::Day => ibapi::orders::TimeInForce::Day,
            Self::GoodTilCanceled => ibapi::orders::TimeInForce::GoodTilCanceled,
            Self::ImmediateOrCancel => ibapi::orders::TimeInForce::ImmediateOrCancel,
            Self::GoodTilDate => ibapi::orders::TimeInForce::GoodTilDate,
            Self::OnOpen => ibapi::orders::TimeInForce::OnOpen,
            Self::FillOrKill => ibapi::orders::TimeInForce::FillOrKill,
            Self::DayTilCanceled => ibapi::orders::TimeInForce::DayTilCanceled,
            Self::Auction => ibapi::orders::TimeInForce::Auction,
        }
    }
}

impl From<ibapi::orders::TimeInForce> for IbTimeInForce {
    fn from(value: ibapi::orders::TimeInForce) -> Self {
        match value {
            ibapi::orders::TimeInForce::Day => Self::Day,
            ibapi::orders::TimeInForce::GoodTilCanceled => Self::GoodTilCanceled,
            ibapi::orders::TimeInForce::ImmediateOrCancel => Self::ImmediateOrCancel,
            ibapi::orders::TimeInForce::GoodTilDate => Self::GoodTilDate,
            ibapi::orders::TimeInForce::OnOpen => Self::OnOpen,
            ibapi::orders::TimeInForce::FillOrKill => Self::FillOrKill,
            ibapi::orders::TimeInForce::DayTilCanceled => Self::DayTilCanceled,
            ibapi::orders::TimeInForce::Auction => Self::Auction,
        }
    }
}

impl FromStr for IbTimeInForce {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "DAY" => Ok(Self::Day),
            "GTC" => Ok(Self::GoodTilCanceled),
            "IOC" => Ok(Self::ImmediateOrCancel),
            "GTD" => Ok(Self::GoodTilDate),
            "OPG" => Ok(Self::OnOpen),
            "FOK" => Ok(Self::FillOrKill),
            "DTC" => Ok(Self::DayTilCanceled),
            "AUC" => Ok(Self::Auction),
            _ => anyhow::bail!("Unknown IB time in force: {value}"),
        }
    }
}

impl Display for IbTimeInForce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Day => "DAY",
            Self::GoodTilCanceled => "GTC",
            Self::ImmediateOrCancel => "IOC",
            Self::GoodTilDate => "GTD",
            Self::OnOpen => "OPG",
            Self::FillOrKill => "FOK",
            Self::DayTilCanceled => "DTC",
            Self::Auction => "AUC",
        })
    }
}

/// Interactive Brokers builder time-in-force values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbBuilderTimeInForce {
    Day,
    GoodTillCancel,
    ImmediateOrCancel,
    GoodTillDate,
    FillOrKill,
    GoodTillCrossing,
    DayTillCanceled,
    Auction,
    OpeningAuction,
}

impl IbBuilderTimeInForce {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "DAY",
            Self::GoodTillCancel => "GTC",
            Self::ImmediateOrCancel => "IOC",
            Self::GoodTillDate => "GTD",
            Self::FillOrKill => "FOK",
            Self::GoodTillCrossing => "GTX",
            Self::DayTillCanceled => "DTC",
            Self::Auction => "AUC",
            Self::OpeningAuction => "OPG",
        }
    }

    #[must_use]
    pub fn ibapi_builder_time_in_force(
        self,
        good_till_date: Option<String>,
    ) -> ibapi::orders::builder::TimeInForce {
        match self {
            Self::Day => ibapi::orders::builder::TimeInForce::Day,
            Self::GoodTillCancel => ibapi::orders::builder::TimeInForce::GoodTillCancel,
            Self::ImmediateOrCancel => ibapi::orders::builder::TimeInForce::ImmediateOrCancel,
            Self::GoodTillDate => ibapi::orders::builder::TimeInForce::GoodTillDate {
                date: good_till_date.unwrap_or_default(),
            },
            Self::FillOrKill => ibapi::orders::builder::TimeInForce::FillOrKill,
            Self::GoodTillCrossing => ibapi::orders::builder::TimeInForce::GoodTillCrossing,
            Self::DayTillCanceled => ibapi::orders::builder::TimeInForce::DayTillCanceled,
            Self::Auction => ibapi::orders::builder::TimeInForce::Auction,
            Self::OpeningAuction => ibapi::orders::builder::TimeInForce::OpeningAuction,
        }
    }
}

impl FromStr for IbBuilderTimeInForce {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "DAY" => Ok(Self::Day),
            "GTC" => Ok(Self::GoodTillCancel),
            "IOC" => Ok(Self::ImmediateOrCancel),
            "GTD" => Ok(Self::GoodTillDate),
            "FOK" => Ok(Self::FillOrKill),
            "GTX" => Ok(Self::GoodTillCrossing),
            "DTC" => Ok(Self::DayTillCanceled),
            "AUC" => Ok(Self::Auction),
            "OPG" => Ok(Self::OpeningAuction),
            _ => anyhow::bail!("Unknown IB builder time in force: {value}"),
        }
    }
}

impl Display for IbBuilderTimeInForce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers security type values used by the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbSecurityType {
    /// Stock or ETF.
    Stock,
    /// Equity/index option.
    Option,
    /// Future.
    Future,
    /// Continuous future.
    ContinuousFuture,
    /// Index.
    Index,
    /// Futures option.
    FuturesOption,
    /// Forex pair.
    ForexPair,
    /// Combo/spread.
    Spread,
    /// Warrant.
    Warrant,
    /// Bond.
    Bond,
    /// Commodity.
    Commodity,
    /// News.
    News,
    /// Mutual fund.
    MutualFund,
    /// Crypto currency.
    Crypto,
    /// Contract for difference.
    Cfd,
}

impl IbSecurityType {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stock => "STK",
            Self::Option => "OPT",
            Self::Future => "FUT",
            Self::ContinuousFuture => "CONTFUT",
            Self::Index => "IND",
            Self::FuturesOption => "FOP",
            Self::ForexPair => "CASH",
            Self::Spread => "BAG",
            Self::Warrant => "WAR",
            Self::Bond => "BOND",
            Self::Commodity => "CMDTY",
            Self::News => "NEWS",
            Self::MutualFund => "FUND",
            Self::Crypto => "CRYPTO",
            Self::Cfd => "CFD",
        }
    }

    /// Converts to the rust-ibapi security type.
    #[must_use]
    pub const fn ibapi_security_type(self) -> ibapi::contracts::SecurityType {
        match self {
            Self::Stock => ibapi::contracts::SecurityType::Stock,
            Self::Option => ibapi::contracts::SecurityType::Option,
            Self::Future => ibapi::contracts::SecurityType::Future,
            Self::ContinuousFuture => ibapi::contracts::SecurityType::ContinuousFuture,
            Self::Index => ibapi::contracts::SecurityType::Index,
            Self::FuturesOption => ibapi::contracts::SecurityType::FuturesOption,
            Self::ForexPair => ibapi::contracts::SecurityType::ForexPair,
            Self::Spread => ibapi::contracts::SecurityType::Spread,
            Self::Warrant => ibapi::contracts::SecurityType::Warrant,
            Self::Bond => ibapi::contracts::SecurityType::Bond,
            Self::Commodity => ibapi::contracts::SecurityType::Commodity,
            Self::News => ibapi::contracts::SecurityType::News,
            Self::MutualFund => ibapi::contracts::SecurityType::MutualFund,
            Self::Crypto => ibapi::contracts::SecurityType::Crypto,
            Self::Cfd => ibapi::contracts::SecurityType::CFD,
        }
    }
}

impl TryFrom<&ibapi::contracts::SecurityType> for IbSecurityType {
    type Error = anyhow::Error;

    fn try_from(value: &ibapi::contracts::SecurityType) -> Result<Self, Self::Error> {
        match value {
            ibapi::contracts::SecurityType::Stock => Ok(Self::Stock),
            ibapi::contracts::SecurityType::Option => Ok(Self::Option),
            ibapi::contracts::SecurityType::Future => Ok(Self::Future),
            ibapi::contracts::SecurityType::ContinuousFuture => Ok(Self::ContinuousFuture),
            ibapi::contracts::SecurityType::Index => Ok(Self::Index),
            ibapi::contracts::SecurityType::FuturesOption => Ok(Self::FuturesOption),
            ibapi::contracts::SecurityType::ForexPair => Ok(Self::ForexPair),
            ibapi::contracts::SecurityType::Spread => Ok(Self::Spread),
            ibapi::contracts::SecurityType::Warrant => Ok(Self::Warrant),
            ibapi::contracts::SecurityType::Bond => Ok(Self::Bond),
            ibapi::contracts::SecurityType::Commodity => Ok(Self::Commodity),
            ibapi::contracts::SecurityType::News => Ok(Self::News),
            ibapi::contracts::SecurityType::MutualFund => Ok(Self::MutualFund),
            ibapi::contracts::SecurityType::Crypto => Ok(Self::Crypto),
            ibapi::contracts::SecurityType::CFD => Ok(Self::Cfd),
            ibapi::contracts::SecurityType::Other(value) => {
                anyhow::bail!("Unknown IB security type: {value}")
            }
        }
    }
}

impl FromStr for IbSecurityType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_uppercase().as_str() {
            "STK" => Ok(Self::Stock),
            "OPT" => Ok(Self::Option),
            "FUT" => Ok(Self::Future),
            "CONTFUT" => Ok(Self::ContinuousFuture),
            "IND" => Ok(Self::Index),
            "FOP" => Ok(Self::FuturesOption),
            "CASH" => Ok(Self::ForexPair),
            "BAG" => Ok(Self::Spread),
            "WAR" => Ok(Self::Warrant),
            "BOND" => Ok(Self::Bond),
            "CMDTY" => Ok(Self::Commodity),
            "NEWS" => Ok(Self::News),
            "FUND" => Ok(Self::MutualFund),
            "CRYPTO" => Ok(Self::Crypto),
            "CFD" => Ok(Self::Cfd),
            _ => anyhow::bail!("Unknown IB security type: {value}"),
        }
    }
}

impl Display for IbSecurityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers option right values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOptionRight {
    /// Call option.
    Call,
    /// Put option.
    Put,
}

impl IbOptionRight {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Call => "C",
            Self::Put => "P",
        }
    }

    /// Converts this option right to a Nautilus option kind.
    #[must_use]
    pub const fn option_kind(self) -> OptionKind {
        match self {
            Self::Call => OptionKind::Call,
            Self::Put => OptionKind::Put,
        }
    }
}

impl FromStr for IbOptionRight {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_uppercase().as_str() {
            "C" | "CALL" => Ok(Self::Call),
            "P" | "PUT" => Ok(Self::Put),
            _ => anyhow::bail!("Unknown IB option right: {value}"),
        }
    }
}

impl Display for IbOptionRight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers historical tick request types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbHistoricalTickType {
    /// Historical trade ticks.
    Trades,
    /// Historical bid/ask ticks.
    BidAsk,
}

impl IbHistoricalTickType {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "TRADES",
            Self::BidAsk => "BID_ASK",
        }
    }
}

impl FromStr for IbHistoricalTickType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_uppercase().as_str() {
            "TRADES" => Ok(Self::Trades),
            "BID_ASK" => Ok(Self::BidAsk),
            _ => anyhow::bail!("Unknown IB historical tick type: {value}"),
        }
    }
}

impl Display for IbHistoricalTickType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers trading hours selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTradingHours {
    /// Regular trading hours only.
    Regular,
    /// Include extended trading hours.
    Extended,
}

impl IbTradingHours {
    /// Returns whether IB should use regular trading hours only.
    #[must_use]
    pub const fn use_rth(self) -> bool {
        matches!(self, Self::Regular)
    }

    /// Converts to the rust-ibapi trading hours enum.
    #[must_use]
    pub const fn ibapi_trading_hours(self) -> ibapi::market_data::TradingHours {
        match self {
            Self::Regular => ibapi::market_data::TradingHours::Regular,
            Self::Extended => ibapi::market_data::TradingHours::Extended,
        }
    }
}

impl From<bool> for IbTradingHours {
    fn from(use_rth: bool) -> Self {
        if use_rth {
            Self::Regular
        } else {
            Self::Extended
        }
    }
}

/// Interactive Brokers historical bar-size values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbHistoricalBarSize {
    Sec,
    Sec5,
    Sec10,
    Sec15,
    Sec30,
    Min,
    Min2,
    Min3,
    Min5,
    Min10,
    Min15,
    Min20,
    Min30,
    Hour,
    Hour2,
    Hour3,
    Hour4,
    Hour8,
    Day,
    Week,
    Month,
}

impl IbHistoricalBarSize {
    /// Converts to the rust-ibapi historical bar-size enum.
    #[must_use]
    pub const fn ibapi_bar_size(self) -> ibapi::market_data::historical::BarSize {
        match self {
            Self::Sec => ibapi::market_data::historical::BarSize::Sec,
            Self::Sec5 => ibapi::market_data::historical::BarSize::Sec5,
            Self::Sec10 => ibapi::market_data::historical::BarSize::Sec10,
            Self::Sec15 => ibapi::market_data::historical::BarSize::Sec15,
            Self::Sec30 => ibapi::market_data::historical::BarSize::Sec30,
            Self::Min => ibapi::market_data::historical::BarSize::Min,
            Self::Min2 => ibapi::market_data::historical::BarSize::Min2,
            Self::Min3 => ibapi::market_data::historical::BarSize::Min3,
            Self::Min5 => ibapi::market_data::historical::BarSize::Min5,
            Self::Min10 => ibapi::market_data::historical::BarSize::Min10,
            Self::Min15 => ibapi::market_data::historical::BarSize::Min15,
            Self::Min20 => ibapi::market_data::historical::BarSize::Min20,
            Self::Min30 => ibapi::market_data::historical::BarSize::Min30,
            Self::Hour => ibapi::market_data::historical::BarSize::Hour,
            Self::Hour2 => ibapi::market_data::historical::BarSize::Hour2,
            Self::Hour3 => ibapi::market_data::historical::BarSize::Hour3,
            Self::Hour4 => ibapi::market_data::historical::BarSize::Hour4,
            Self::Hour8 => ibapi::market_data::historical::BarSize::Hour8,
            Self::Day => ibapi::market_data::historical::BarSize::Day,
            Self::Week => ibapi::market_data::historical::BarSize::Week,
            Self::Month => ibapi::market_data::historical::BarSize::Month,
        }
    }
}

impl Display for IbHistoricalBarSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.ibapi_bar_size())
    }
}

/// Interactive Brokers historical data selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbHistoricalWhatToShow {
    Trades,
    Midpoint,
    Bid,
    Ask,
    BidAsk,
    HistoricalVolatility,
    OptionImpliedVolatility,
    FeeRate,
    Schedule,
    AdjustedLast,
}

impl IbHistoricalWhatToShow {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "TRADES",
            Self::Midpoint => "MIDPOINT",
            Self::Bid => "BID",
            Self::Ask => "ASK",
            Self::BidAsk => "BID_ASK",
            Self::HistoricalVolatility => "HISTORICAL_VOLATILITY",
            Self::OptionImpliedVolatility => "OPTION_IMPLIED_VOLATILITY",
            Self::FeeRate => "FEE_RATE",
            Self::Schedule => "SCHEDULE",
            Self::AdjustedLast => "ADJUSTED_LAST",
        }
    }

    /// Converts to the rust-ibapi historical data selector.
    #[must_use]
    pub const fn ibapi_what_to_show(self) -> ibapi::market_data::historical::WhatToShow {
        match self {
            Self::Trades => ibapi::market_data::historical::WhatToShow::Trades,
            Self::Midpoint => ibapi::market_data::historical::WhatToShow::MidPoint,
            Self::Bid => ibapi::market_data::historical::WhatToShow::Bid,
            Self::Ask => ibapi::market_data::historical::WhatToShow::Ask,
            Self::BidAsk => ibapi::market_data::historical::WhatToShow::BidAsk,
            Self::HistoricalVolatility => {
                ibapi::market_data::historical::WhatToShow::HistoricalVolatility
            }
            Self::OptionImpliedVolatility => {
                ibapi::market_data::historical::WhatToShow::OptionImpliedVolatility
            }
            Self::FeeRate => ibapi::market_data::historical::WhatToShow::FeeRate,
            Self::Schedule => ibapi::market_data::historical::WhatToShow::Schedule,
            Self::AdjustedLast => ibapi::market_data::historical::WhatToShow::AdjustedLast,
        }
    }
}

impl Display for IbHistoricalWhatToShow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers realtime bar-size values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbRealtimeBarSize {
    Sec5,
}

impl IbRealtimeBarSize {
    /// Converts to the rust-ibapi realtime bar-size enum.
    #[must_use]
    pub const fn ibapi_bar_size(self) -> ibapi::market_data::realtime::BarSize {
        match self {
            Self::Sec5 => ibapi::market_data::realtime::BarSize::Sec5,
        }
    }
}

impl Display for IbRealtimeBarSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sec5 => f.write_str("5 secs"),
        }
    }
}

/// Interactive Brokers realtime bar selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbRealtimeWhatToShow {
    Trades,
    Midpoint,
    Bid,
    Ask,
}

impl IbRealtimeWhatToShow {
    /// Returns the IB wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "TRADES",
            Self::Midpoint => "MIDPOINT",
            Self::Bid => "BID",
            Self::Ask => "ASK",
        }
    }

    /// Converts to the rust-ibapi realtime data selector.
    #[must_use]
    pub const fn ibapi_what_to_show(self) -> ibapi::market_data::realtime::WhatToShow {
        match self {
            Self::Trades => ibapi::market_data::realtime::WhatToShow::Trades,
            Self::Midpoint => ibapi::market_data::realtime::WhatToShow::MidPoint,
            Self::Bid => ibapi::market_data::realtime::WhatToShow::Bid,
            Self::Ask => ibapi::market_data::realtime::WhatToShow::Ask,
        }
    }
}

impl Display for IbRealtimeWhatToShow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers combo-leg open/close values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbComboLegOpenClose {
    Same,
    Open,
    Close,
    Unknown,
}

impl IbComboLegOpenClose {
    /// Returns the IB integer code.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Same => 0,
            Self::Open => 1,
            Self::Close => 2,
            Self::Unknown => 3,
        }
    }

    /// Converts to the rust-ibapi combo-leg open/close enum.
    #[must_use]
    pub const fn ibapi_combo_leg_open_close(self) -> ibapi::contracts::ComboLegOpenClose {
        match self {
            Self::Same => ibapi::contracts::ComboLegOpenClose::Same,
            Self::Open => ibapi::contracts::ComboLegOpenClose::Open,
            Self::Close => ibapi::contracts::ComboLegOpenClose::Close,
            Self::Unknown => ibapi::contracts::ComboLegOpenClose::Unknown,
        }
    }
}

impl From<ibapi::contracts::ComboLegOpenClose> for IbComboLegOpenClose {
    fn from(value: ibapi::contracts::ComboLegOpenClose) -> Self {
        match value {
            ibapi::contracts::ComboLegOpenClose::Same => Self::Same,
            ibapi::contracts::ComboLegOpenClose::Open => Self::Open,
            ibapi::contracts::ComboLegOpenClose::Close => Self::Close,
            ibapi::contracts::ComboLegOpenClose::Unknown => Self::Unknown,
        }
    }
}

/// Interactive Brokers conditional order condition types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbConditionKind {
    /// Price condition.
    Price,
    /// Time condition.
    Time,
    /// Margin condition.
    Margin,
    /// Execution condition.
    Execution,
    /// Volume condition.
    Volume,
    /// Percent-change condition.
    PercentChange,
}

impl IbConditionKind {
    /// Returns the JSON/tag string accepted by the adapter.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Price => "price",
            Self::Time => "time",
            Self::Margin => "margin",
            Self::Execution => "execution",
            Self::Volume => "volume",
            Self::PercentChange => "percent_change",
        }
    }
}

impl FromStr for IbConditionKind {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "price" => Ok(Self::Price),
            "time" => Ok(Self::Time),
            "margin" => Ok(Self::Margin),
            "execution" => Ok(Self::Execution),
            "volume" => Ok(Self::Volume),
            "percent_change" | "percentchange" => Ok(Self::PercentChange),
            _ => anyhow::bail!("Unknown IB condition kind: {value}"),
        }
    }
}

impl Display for IbConditionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers conditional order conjunction values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbConditionConjunction {
    /// Logical AND.
    And,
    /// Logical OR.
    Or,
}

impl IbConditionConjunction {
    /// Returns the JSON/tag string accepted by the adapter.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
        }
    }

    /// Returns whether this conjunction is the rust-ibapi `is_conjunction` flag.
    #[must_use]
    pub const fn is_conjunction(self) -> bool {
        matches!(self, Self::And)
    }
}

impl FromStr for IbConditionConjunction {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "and" | "a" => Ok(Self::And),
            "or" | "o" => Ok(Self::Or),
            _ => anyhow::bail!("Unknown IB condition conjunction: {value}"),
        }
    }
}

impl Display for IbConditionConjunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Interactive Brokers price-condition trigger methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTriggerMethod {
    /// Default method.
    Default,
    /// Two consecutive bid or ask prices.
    DoubleBidAsk,
    /// Last traded price.
    Last,
    /// Two consecutive last prices.
    DoubleLast,
    /// Current bid or ask price.
    BidAsk,
    /// Last price or bid/ask if no last price is available.
    LastOrBidAsk,
    /// Midpoint between bid and ask.
    Midpoint,
}

impl IbTriggerMethod {
    /// Returns the IB integer code.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Default => 0,
            Self::DoubleBidAsk => 1,
            Self::Last => 2,
            Self::DoubleLast => 3,
            Self::BidAsk => 4,
            Self::LastOrBidAsk => 7,
            Self::Midpoint => 8,
        }
    }

    /// Converts to the rust-ibapi trigger method.
    #[must_use]
    pub const fn ibapi_trigger_method(self) -> ibapi::orders::conditions::TriggerMethod {
        match self {
            Self::Default => ibapi::orders::conditions::TriggerMethod::Default,
            Self::DoubleBidAsk => ibapi::orders::conditions::TriggerMethod::DoubleBidAsk,
            Self::Last => ibapi::orders::conditions::TriggerMethod::Last,
            Self::DoubleLast => ibapi::orders::conditions::TriggerMethod::DoubleLast,
            Self::BidAsk => ibapi::orders::conditions::TriggerMethod::BidAsk,
            Self::LastOrBidAsk => ibapi::orders::conditions::TriggerMethod::LastOrBidAsk,
            Self::Midpoint => ibapi::orders::conditions::TriggerMethod::Midpoint,
        }
    }
}

impl From<i32> for IbTriggerMethod {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::DoubleBidAsk,
            2 => Self::Last,
            3 => Self::DoubleLast,
            4 => Self::BidAsk,
            7 => Self::LastOrBidAsk,
            8 => Self::Midpoint,
            _ => Self::Default,
        }
    }
}

impl From<IbTriggerMethod> for i32 {
    fn from(value: IbTriggerMethod) -> Self {
        value.as_i32()
    }
}

impl From<ibapi::orders::conditions::TriggerMethod> for IbTriggerMethod {
    fn from(value: ibapi::orders::conditions::TriggerMethod) -> Self {
        i32::from(value).into()
    }
}

impl Display for IbTriggerMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_i32())
    }
}

/// Interactive Brokers one-cancels-all behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOcaType {
    /// Not part of an OCA group.
    None,
    /// Cancel remaining orders with block.
    CancelWithBlock,
    /// Proportionally reduce remaining orders with block.
    ReduceWithBlock,
    /// Proportionally reduce remaining orders without block.
    ReduceWithoutBlock,
}

impl IbOcaType {
    /// Returns the IB integer code.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::None => 0,
            Self::CancelWithBlock => 1,
            Self::ReduceWithBlock => 2,
            Self::ReduceWithoutBlock => 3,
        }
    }

    /// Converts to the rust-ibapi OCA type.
    #[must_use]
    pub const fn ibapi_oca_type(self) -> ibapi::orders::OcaType {
        match self {
            Self::None => ibapi::orders::OcaType::None,
            Self::CancelWithBlock => ibapi::orders::OcaType::CancelWithBlock,
            Self::ReduceWithBlock => ibapi::orders::OcaType::ReduceWithBlock,
            Self::ReduceWithoutBlock => ibapi::orders::OcaType::ReduceWithoutBlock,
        }
    }
}

impl From<i32> for IbOcaType {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::CancelWithBlock,
            2 => Self::ReduceWithBlock,
            3 => Self::ReduceWithoutBlock,
            _ => Self::None,
        }
    }
}

impl From<IbOcaType> for i32 {
    fn from(value: IbOcaType) -> Self {
        value.as_i32()
    }
}

impl From<ibapi::orders::OcaType> for IbOcaType {
    fn from(value: ibapi::orders::OcaType) -> Self {
        i32::from(value).into()
    }
}

impl Display for IbOcaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_i32())
    }
}

/// Interactive Brokers execution liquidity values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbLiquidity {
    None,
    AddedLiquidity,
    RemovedLiquidity,
    LiquidityRoutedOut,
}

impl IbLiquidity {
    /// Returns the IB integer code.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::None => 0,
            Self::AddedLiquidity => 1,
            Self::RemovedLiquidity => 2,
            Self::LiquidityRoutedOut => 3,
        }
    }

    /// Converts to the rust-ibapi execution liquidity enum.
    #[must_use]
    pub fn ibapi_liquidity(self) -> ibapi::orders::Liquidity {
        ibapi::orders::Liquidity::from(self.as_i32())
    }
}

impl From<i32> for IbLiquidity {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::AddedLiquidity,
            2 => Self::RemovedLiquidity,
            3 => Self::LiquidityRoutedOut,
            _ => Self::None,
        }
    }
}

impl From<ibapi::orders::Liquidity> for IbLiquidity {
    fn from(value: ibapi::orders::Liquidity) -> Self {
        match value {
            ibapi::orders::Liquidity::None => Self::None,
            ibapi::orders::Liquidity::AddedLiquidity => Self::AddedLiquidity,
            ibapi::orders::Liquidity::RemovedLiquidity => Self::RemovedLiquidity,
            ibapi::orders::Liquidity::LiquidityRoutedOut => Self::LiquidityRoutedOut,
        }
    }
}

impl Display for IbLiquidity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_i32())
    }
}

/// Interactive Brokers market data tick types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTickType {
    Unknown,
    BidSize,
    Bid,
    Ask,
    AskSize,
    Last,
    LastSize,
    High,
    Low,
    Volume,
    Close,
    BidOption,
    AskOption,
    LastOption,
    ModelOption,
    Open,
    Low13Week,
    High13Week,
    Low26Week,
    High26Week,
    Low52Week,
    High52Week,
    AvgVolume,
    OpenInterest,
    OptionHistoricalVol,
    OptionImpliedVol,
    OptionBidExch,
    OptionAskExch,
    OptionCallOpenInterest,
    OptionPutOpenInterest,
    OptionCallVolume,
    OptionPutVolume,
    IndexFuturePremium,
    BidExch,
    AskExch,
    AuctionVolume,
    AuctionPrice,
    AuctionImbalance,
    MarkPrice,
    BidEfpComputation,
    AskEfpComputation,
    LastEfpComputation,
    OpenEfpComputation,
    HighEfpComputation,
    LowEfpComputation,
    CloseEfpComputation,
    LastTimestamp,
    Shortable,
    FundamentalRatios,
    RtVolume,
    Halted,
    BidYield,
    AskYield,
    LastYield,
    CustOptionComputation,
    TradeCount,
    TradeRate,
    VolumeRate,
    LastRthTrade,
    RtHistoricalVol,
    IbDividends,
    BondFactorMultiplier,
    RegulatoryImbalance,
    NewsTick,
    ShortTermVolume3Min,
    ShortTermVolume5Min,
    ShortTermVolume10Min,
    DelayedBid,
    DelayedAsk,
    DelayedLast,
    DelayedBidSize,
    DelayedAskSize,
    DelayedLastSize,
    DelayedHigh,
    DelayedLow,
    DelayedVolume,
    DelayedClose,
    DelayedOpen,
    RtTrdVolume,
    CreditmanMarkPrice,
    CreditmanSlowMarkPrice,
    DelayedBidOption,
    DelayedAskOption,
    DelayedLastOption,
    DelayedModelOption,
    LastExch,
    LastRegTime,
    FuturesOpenInterest,
    AvgOptVolume,
    DelayedLastTimestamp,
    ShortableShares,
    DelayedHalted,
    Reuters2MutualFunds,
    EtfNavClose,
    EtfNavPriorClose,
    EtfNavBid,
    EtfNavAsk,
    EtfNavLast,
    EtfFrozenNavLast,
    EtfNavHigh,
    EtfNavLow,
    SocialMarketAnalytics,
    EstimatedIpoMidpoint,
    FinalIpoLast,
    DelayedYieldBid,
    DelayedYieldAsk,
}

impl IbTickType {
    /// Returns the IB integer code.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Unknown => -1,
            Self::BidSize => 0,
            Self::Bid => 1,
            Self::Ask => 2,
            Self::AskSize => 3,
            Self::Last => 4,
            Self::LastSize => 5,
            Self::High => 6,
            Self::Low => 7,
            Self::Volume => 8,
            Self::Close => 9,
            Self::BidOption => 10,
            Self::AskOption => 11,
            Self::LastOption => 12,
            Self::ModelOption => 13,
            Self::Open => 14,
            Self::Low13Week => 15,
            Self::High13Week => 16,
            Self::Low26Week => 17,
            Self::High26Week => 18,
            Self::Low52Week => 19,
            Self::High52Week => 20,
            Self::AvgVolume => 21,
            Self::OpenInterest => 22,
            Self::OptionHistoricalVol => 23,
            Self::OptionImpliedVol => 24,
            Self::OptionBidExch => 25,
            Self::OptionAskExch => 26,
            Self::OptionCallOpenInterest => 27,
            Self::OptionPutOpenInterest => 28,
            Self::OptionCallVolume => 29,
            Self::OptionPutVolume => 30,
            Self::IndexFuturePremium => 31,
            Self::BidExch => 32,
            Self::AskExch => 33,
            Self::AuctionVolume => 34,
            Self::AuctionPrice => 35,
            Self::AuctionImbalance => 36,
            Self::MarkPrice => 37,
            Self::BidEfpComputation => 38,
            Self::AskEfpComputation => 39,
            Self::LastEfpComputation => 40,
            Self::OpenEfpComputation => 41,
            Self::HighEfpComputation => 42,
            Self::LowEfpComputation => 43,
            Self::CloseEfpComputation => 44,
            Self::LastTimestamp => 45,
            Self::Shortable => 46,
            Self::FundamentalRatios => 47,
            Self::RtVolume => 48,
            Self::Halted => 49,
            Self::BidYield => 50,
            Self::AskYield => 51,
            Self::LastYield => 52,
            Self::CustOptionComputation => 53,
            Self::TradeCount => 54,
            Self::TradeRate => 55,
            Self::VolumeRate => 56,
            Self::LastRthTrade => 57,
            Self::RtHistoricalVol => 58,
            Self::IbDividends => 59,
            Self::BondFactorMultiplier => 60,
            Self::RegulatoryImbalance => 61,
            Self::NewsTick => 62,
            Self::ShortTermVolume3Min => 63,
            Self::ShortTermVolume5Min => 64,
            Self::ShortTermVolume10Min => 65,
            Self::DelayedBid => 66,
            Self::DelayedAsk => 67,
            Self::DelayedLast => 68,
            Self::DelayedBidSize => 69,
            Self::DelayedAskSize => 70,
            Self::DelayedLastSize => 71,
            Self::DelayedHigh => 72,
            Self::DelayedLow => 73,
            Self::DelayedVolume => 74,
            Self::DelayedClose => 75,
            Self::DelayedOpen => 76,
            Self::RtTrdVolume => 77,
            Self::CreditmanMarkPrice => 78,
            Self::CreditmanSlowMarkPrice => 79,
            Self::DelayedBidOption => 80,
            Self::DelayedAskOption => 81,
            Self::DelayedLastOption => 82,
            Self::DelayedModelOption => 83,
            Self::LastExch => 84,
            Self::LastRegTime => 85,
            Self::FuturesOpenInterest => 86,
            Self::AvgOptVolume => 87,
            Self::DelayedLastTimestamp => 88,
            Self::ShortableShares => 89,
            Self::DelayedHalted => 90,
            Self::Reuters2MutualFunds => 91,
            Self::EtfNavClose => 92,
            Self::EtfNavPriorClose => 93,
            Self::EtfNavBid => 94,
            Self::EtfNavAsk => 95,
            Self::EtfNavLast => 96,
            Self::EtfFrozenNavLast => 97,
            Self::EtfNavHigh => 98,
            Self::EtfNavLow => 99,
            Self::SocialMarketAnalytics => 100,
            Self::EstimatedIpoMidpoint => 101,
            Self::FinalIpoLast => 102,
            Self::DelayedYieldBid => 103,
            Self::DelayedYieldAsk => 104,
        }
    }

    /// Converts to the rust-ibapi tick type enum.
    #[must_use]
    pub fn ibapi_tick_type(self) -> ibapi::contracts::tick_types::TickType {
        ibapi::contracts::tick_types::TickType::from(self.as_i32())
    }
}

impl From<i32> for IbTickType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::BidSize,
            1 => Self::Bid,
            2 => Self::Ask,
            3 => Self::AskSize,
            4 => Self::Last,
            5 => Self::LastSize,
            6 => Self::High,
            7 => Self::Low,
            8 => Self::Volume,
            9 => Self::Close,
            10 => Self::BidOption,
            11 => Self::AskOption,
            12 => Self::LastOption,
            13 => Self::ModelOption,
            14 => Self::Open,
            15 => Self::Low13Week,
            16 => Self::High13Week,
            17 => Self::Low26Week,
            18 => Self::High26Week,
            19 => Self::Low52Week,
            20 => Self::High52Week,
            21 => Self::AvgVolume,
            22 => Self::OpenInterest,
            23 => Self::OptionHistoricalVol,
            24 => Self::OptionImpliedVol,
            25 => Self::OptionBidExch,
            26 => Self::OptionAskExch,
            27 => Self::OptionCallOpenInterest,
            28 => Self::OptionPutOpenInterest,
            29 => Self::OptionCallVolume,
            30 => Self::OptionPutVolume,
            31 => Self::IndexFuturePremium,
            32 => Self::BidExch,
            33 => Self::AskExch,
            34 => Self::AuctionVolume,
            35 => Self::AuctionPrice,
            36 => Self::AuctionImbalance,
            37 => Self::MarkPrice,
            38 => Self::BidEfpComputation,
            39 => Self::AskEfpComputation,
            40 => Self::LastEfpComputation,
            41 => Self::OpenEfpComputation,
            42 => Self::HighEfpComputation,
            43 => Self::LowEfpComputation,
            44 => Self::CloseEfpComputation,
            45 => Self::LastTimestamp,
            46 => Self::Shortable,
            47 => Self::FundamentalRatios,
            48 => Self::RtVolume,
            49 => Self::Halted,
            50 => Self::BidYield,
            51 => Self::AskYield,
            52 => Self::LastYield,
            53 => Self::CustOptionComputation,
            54 => Self::TradeCount,
            55 => Self::TradeRate,
            56 => Self::VolumeRate,
            57 => Self::LastRthTrade,
            58 => Self::RtHistoricalVol,
            59 => Self::IbDividends,
            60 => Self::BondFactorMultiplier,
            61 => Self::RegulatoryImbalance,
            62 => Self::NewsTick,
            63 => Self::ShortTermVolume3Min,
            64 => Self::ShortTermVolume5Min,
            65 => Self::ShortTermVolume10Min,
            66 => Self::DelayedBid,
            67 => Self::DelayedAsk,
            68 => Self::DelayedLast,
            69 => Self::DelayedBidSize,
            70 => Self::DelayedAskSize,
            71 => Self::DelayedLastSize,
            72 => Self::DelayedHigh,
            73 => Self::DelayedLow,
            74 => Self::DelayedVolume,
            75 => Self::DelayedClose,
            76 => Self::DelayedOpen,
            77 => Self::RtTrdVolume,
            78 => Self::CreditmanMarkPrice,
            79 => Self::CreditmanSlowMarkPrice,
            80 => Self::DelayedBidOption,
            81 => Self::DelayedAskOption,
            82 => Self::DelayedLastOption,
            83 => Self::DelayedModelOption,
            84 => Self::LastExch,
            85 => Self::LastRegTime,
            86 => Self::FuturesOpenInterest,
            87 => Self::AvgOptVolume,
            88 => Self::DelayedLastTimestamp,
            89 => Self::ShortableShares,
            90 => Self::DelayedHalted,
            91 => Self::Reuters2MutualFunds,
            92 => Self::EtfNavClose,
            93 => Self::EtfNavPriorClose,
            94 => Self::EtfNavBid,
            95 => Self::EtfNavAsk,
            96 => Self::EtfNavLast,
            97 => Self::EtfFrozenNavLast,
            98 => Self::EtfNavHigh,
            99 => Self::EtfNavLow,
            100 => Self::SocialMarketAnalytics,
            101 => Self::EstimatedIpoMidpoint,
            102 => Self::FinalIpoLast,
            103 => Self::DelayedYieldBid,
            104 => Self::DelayedYieldAsk,
            _ => Self::Unknown,
        }
    }
}

impl Display for IbTickType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_i32())
    }
}

macro_rules! define_ib_i32_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $value:expr
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[cfg_attr(
            feature = "python",
            pyo3::pyclass(
                module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
                from_py_object
            )
        )]
        pub enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            #[must_use]
            pub const fn as_i32(self) -> i32 {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl From<i32> for $name {
            fn from(value: i32) -> Self {
                match value {
                    $($value => Self::$variant,)+
                    _ => Self::default(),
                }
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.as_i32())
            }
        }
    };
}

define_ib_i32_enum! {
    /// Interactive Brokers order origin values.
    pub enum IbOrderOrigin {
        #[default]
        Customer = 0,
        Firm = 1,
    }
}

impl IbOrderOrigin {
    #[must_use]
    pub fn ibapi_order_origin(self) -> ibapi::orders::OrderOrigin {
        ibapi::orders::OrderOrigin::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers institutional short-sale slot values.
    pub enum IbShortSaleSlot {
        #[default]
        None = 0,
        Broker = 1,
        ThirdParty = 2,
    }
}

impl IbShortSaleSlot {
    #[must_use]
    pub fn ibapi_short_sale_slot(self) -> ibapi::orders::ShortSaleSlot {
        ibapi::orders::ShortSaleSlot::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers volatility type values.
    pub enum IbVolatilityType {
        #[default]
        Daily = 1,
        Annual = 2,
    }
}

impl IbVolatilityType {
    #[must_use]
    pub fn ibapi_volatility_type(self) -> ibapi::orders::VolatilityType {
        ibapi::orders::VolatilityType::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers VOL order reference price type values.
    pub enum IbReferencePriceType {
        #[default]
        AverageOfNbbo = 1,
        Nbbo = 2,
    }
}

impl IbReferencePriceType {
    #[must_use]
    pub fn ibapi_reference_price_type(self) -> ibapi::orders::ReferencePriceType {
        ibapi::orders::ReferencePriceType::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers BOX auction strategy values.
    pub enum IbAuctionStrategy {
        #[default]
        Match = 1,
        Improvement = 2,
        Transparent = 3,
    }
}

impl IbAuctionStrategy {
    #[must_use]
    pub fn ibapi_auction_strategy(self) -> ibapi::orders::AuctionStrategy {
        ibapi::orders::AuctionStrategy::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers option exercise action values.
    pub enum IbExerciseAction {
        #[default]
        Exercise = 1,
        Lapse = 2,
    }
}

impl IbExerciseAction {
    #[must_use]
    pub const fn ibapi_exercise_action(self) -> ibapi::orders::ExerciseAction {
        match self {
            Self::Exercise => ibapi::orders::ExerciseAction::Exercise,
            Self::Lapse => ibapi::orders::ExerciseAction::Lapse,
        }
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers news article type values.
    pub enum IbArticleType {
        #[default]
        Text = 0,
        Binary = 1,
    }
}

impl IbArticleType {
    #[must_use]
    pub fn ibapi_article_type(self) -> ibapi::news::ArticleType {
        ibapi::news::ArticleType::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers builder auction type values.
    pub enum IbAuctionType {
        #[default]
        Opening = 1,
        Closing = 2,
        Volatility = 4,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbRule80A {
    Individual,
    Agency,
    AgentOtherMember,
    IndividualPtia,
    AgencyPtia,
    AgentOtherMemberPtia,
    IndividualPt,
    AgencyPt,
    AgentOtherMemberPt,
}

impl IbRule80A {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Individual => "I",
            Self::Agency => "A",
            Self::AgentOtherMember => "W",
            Self::IndividualPtia => "J",
            Self::AgencyPtia => "U",
            Self::AgentOtherMemberPtia => "M",
            Self::IndividualPt => "K",
            Self::AgencyPt => "Y",
            Self::AgentOtherMemberPt => "N",
        }
    }
}

impl Display for IbRule80A {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOrderOpenClose {
    Open,
    Close,
}

impl IbOrderOpenClose {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "O",
            Self::Close => "C",
        }
    }
}

impl Display for IbOrderOpenClose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTwapStrategyType {
    Marketable,
    MatchingMidpoint,
    MatchingSameSide,
    MatchingLast,
}

impl IbTwapStrategyType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Marketable => "Marketable",
            Self::MatchingMidpoint => "Matching Midpoint",
            Self::MatchingSameSide => "Matching Same Side",
            Self::MatchingLast => "Matching Last",
        }
    }
}

impl Display for IbTwapStrategyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbRiskAversion {
    GetDone,
    Aggressive,
    Neutral,
    Passive,
}

impl IbRiskAversion {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GetDone => "Get Done",
            Self::Aggressive => "Aggressive",
            Self::Neutral => "Neutral",
            Self::Passive => "Passive",
        }
    }
}

impl Display for IbRiskAversion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbLegAction {
    Buy,
    Sell,
}

impl IbLegAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

impl Display for IbLegAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbFundDistributionPolicyIndicator {
    None,
    AccumulationFund,
    IncomeFund,
}

impl IbFundDistributionPolicyIndicator {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::AccumulationFund => "N",
            Self::IncomeFund => "Y",
        }
    }
}

impl Display for IbFundDistributionPolicyIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbFundAssetType {
    None,
    Others,
    MoneyMarket,
    FixedIncome,
    MultiAsset,
    Equity,
    Sector,
    Guaranteed,
    Alternative,
}

impl IbFundAssetType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Others => "000",
            Self::MoneyMarket => "001",
            Self::FixedIncome => "002",
            Self::MultiAsset => "003",
            Self::Equity => "004",
            Self::Sector => "005",
            Self::Guaranteed => "006",
            Self::Alternative => "007",
        }
    }
}

impl Display for IbFundAssetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Bond identifier discriminator for the rust-ibapi payload enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbBondIdentifierKind {
    Cusip,
    Isin,
}

impl IbBondIdentifierKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cusip => "CUSIP",
            Self::Isin => "ISIN",
        }
    }
}

impl Display for IbBondIdentifierKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Event kind for the rust-ibapi place-order response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbPlaceOrderEvent {
    OrderStatus,
    OpenOrder,
    ExecutionData,
    CommissionReport,
    Message,
}

/// Event kind for the rust-ibapi order-update enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOrderUpdateEvent {
    OrderStatus,
    OpenOrder,
    ExecutionData,
    CommissionReport,
    Message,
}

/// Event kind for the rust-ibapi cancel-order response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbCancelOrderEvent {
    OrderStatus,
    Notice,
}

/// Event kind for the rust-ibapi order query response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOrdersEvent {
    OrderData,
    OrderStatus,
    Notice,
}

/// Event kind for the rust-ibapi executions response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbExecutionsEvent {
    ExecutionData,
    CommissionReport,
    Notice,
}

/// Event kind for the rust-ibapi exercise-options response enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbExerciseOptionsEvent {
    OpenOrder,
    OrderStatus,
    Notice,
}

/// Event kind for rust-ibapi historical bar update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbHistoricalBarUpdateEvent {
    Historical,
    Update,
    End,
}

/// Event kind for rust-ibapi realtime market-depth streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbMarketDepthEvent {
    MarketDepth,
    MarketDepthL2,
    Notice,
}

/// Event kind for rust-ibapi realtime tick streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTickEvent {
    Price,
    Size,
    String,
    Efp,
    Generic,
    OptionComputation,
    SnapshotEnd,
    Notice,
    RequestParameters,
    PriceSize,
}

/// Event kind for rust-ibapi account summary streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbAccountSummaryEvent {
    Summary,
    End,
}

/// Event kind for rust-ibapi position update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbPositionUpdateEvent {
    Position,
    PositionEnd,
}

/// Event kind for rust-ibapi model-code scoped position update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbPositionUpdateMultiEvent {
    Position,
    PositionEnd,
}

/// Event kind for rust-ibapi account update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbAccountUpdateEvent {
    AccountValue,
    PortfolioValue,
    UpdateTime,
    End,
}

/// Event kind for rust-ibapi model-code scoped account update streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbAccountUpdateMultiEvent {
    AccountMultiValue,
    End,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("BUY", IbAction::Buy, OrderSide::Buy, 1)]
    #[case("BOT", IbAction::Bought, OrderSide::Buy, 1)]
    #[case("SELL", IbAction::Sell, OrderSide::Sell, -1)]
    #[case("SLD", IbAction::Sold, OrderSide::Sell, -1)]
    #[case("SSHORT", IbAction::SellShort, OrderSide::Sell, -1)]
    #[case("SLONG", IbAction::SellLong, OrderSide::Sell, -1)]
    fn test_ib_action_parse(
        #[case] value: &str,
        #[case] expected_action: IbAction,
        #[case] expected_side: OrderSide,
        #[case] expected_multiplier: i32,
    ) {
        let action = IbAction::from_str(value).unwrap();
        assert_eq!(action, expected_action);
        assert_eq!(action.order_side(), expected_side);
        assert_eq!(action.signed_multiplier(), expected_multiplier);
        assert_eq!(action.to_string(), value);
        assert_eq!(
            IbAction::from(action.ibapi_action()).order_side(),
            expected_side
        );
    }

    #[rstest]
    #[case(
        "ApiPending",
        IbOrderStatus::ApiPending,
        NautilusOrderStatus::Submitted
    )]
    #[case(
        "PendingSubmit",
        IbOrderStatus::PendingSubmit,
        NautilusOrderStatus::Submitted
    )]
    #[case(
        "PreSubmitted",
        IbOrderStatus::PreSubmitted,
        NautilusOrderStatus::Submitted
    )]
    #[case("Submitted", IbOrderStatus::Submitted, NautilusOrderStatus::Accepted)]
    #[case(
        "PendingCancel",
        IbOrderStatus::PendingCancel,
        NautilusOrderStatus::PendingCancel
    )]
    #[case(
        "ApiCancelled",
        IbOrderStatus::ApiCancelled,
        NautilusOrderStatus::Canceled
    )]
    #[case("Cancelled", IbOrderStatus::Cancelled, NautilusOrderStatus::Canceled)]
    #[case("Filled", IbOrderStatus::Filled, NautilusOrderStatus::Filled)]
    #[case("Inactive", IbOrderStatus::Inactive, NautilusOrderStatus::Rejected)]
    fn test_ib_order_status_parse(
        #[case] value: &str,
        #[case] expected_status: IbOrderStatus,
        #[case] expected_nautilus_status: NautilusOrderStatus,
    ) {
        let status = IbOrderStatus::from_str(value).unwrap();
        assert_eq!(status, expected_status);
        assert_eq!(status.nautilus_status(), expected_nautilus_status);
        assert_eq!(status.to_string(), value);
    }

    #[rstest]
    #[case("MKT", IbOrderType::Market, NautilusOrderType::Market)]
    #[case("MOC", IbOrderType::MarketOnClose, NautilusOrderType::Market)]
    #[case("LMT", IbOrderType::Limit, NautilusOrderType::Limit)]
    #[case("LOC", IbOrderType::LimitOnClose, NautilusOrderType::Limit)]
    #[case("STP", IbOrderType::Stop, NautilusOrderType::StopMarket)]
    #[case("STP LMT", IbOrderType::StopLimit, NautilusOrderType::StopLimit)]
    #[case(
        "TRAIL",
        IbOrderType::TrailingStop,
        NautilusOrderType::TrailingStopMarket
    )]
    #[case(
        "TRAIL LIMIT",
        IbOrderType::TrailingStopLimit,
        NautilusOrderType::TrailingStopLimit
    )]
    #[case(
        "MIT",
        IbOrderType::MarketIfTouched,
        NautilusOrderType::MarketIfTouched
    )]
    #[case("LIT", IbOrderType::LimitIfTouched, NautilusOrderType::LimitIfTouched)]
    #[case("MTL", IbOrderType::MarketToLimit, NautilusOrderType::MarketToLimit)]
    fn test_ib_order_type_parse(
        #[case] value: &str,
        #[case] expected_order_type: IbOrderType,
        #[case] expected_nautilus_order_type: NautilusOrderType,
    ) {
        let order_type = IbOrderType::from_str(value).unwrap();
        assert_eq!(order_type, expected_order_type);
        assert_eq!(
            order_type.nautilus_order_type(),
            expected_nautilus_order_type
        );
        assert_eq!(order_type.to_string(), value);
    }

    #[rstest]
    #[case("DAY", IbTimeInForce::Day, NautilusTimeInForce::Day)]
    #[case("GTC", IbTimeInForce::GoodTilCanceled, NautilusTimeInForce::Gtc)]
    #[case("IOC", IbTimeInForce::ImmediateOrCancel, NautilusTimeInForce::Ioc)]
    #[case("GTD", IbTimeInForce::GoodTilDate, NautilusTimeInForce::Gtd)]
    #[case("OPG", IbTimeInForce::OnOpen, NautilusTimeInForce::AtTheOpen)]
    #[case("FOK", IbTimeInForce::FillOrKill, NautilusTimeInForce::Fok)]
    #[case("DTC", IbTimeInForce::DayTilCanceled, NautilusTimeInForce::Day)]
    #[case("AUC", IbTimeInForce::Auction, NautilusTimeInForce::Day)]
    fn test_ib_time_in_force_parse(
        #[case] value: &str,
        #[case] expected_time_in_force: IbTimeInForce,
        #[case] expected_nautilus_time_in_force: NautilusTimeInForce,
    ) {
        let time_in_force = IbTimeInForce::from_str(value).unwrap();
        assert_eq!(time_in_force, expected_time_in_force);
        assert_eq!(
            time_in_force.nautilus_time_in_force(),
            expected_nautilus_time_in_force
        );
        assert_eq!(time_in_force.to_string(), value);
        assert_eq!(
            IbTimeInForce::from(time_in_force.ibapi_time_in_force()),
            expected_time_in_force
        );
    }

    #[rstest]
    #[case("STK", IbSecurityType::Stock)]
    #[case("OPT", IbSecurityType::Option)]
    #[case("FUT", IbSecurityType::Future)]
    #[case("CONTFUT", IbSecurityType::ContinuousFuture)]
    #[case("IND", IbSecurityType::Index)]
    #[case("FOP", IbSecurityType::FuturesOption)]
    #[case("CASH", IbSecurityType::ForexPair)]
    #[case("BAG", IbSecurityType::Spread)]
    #[case("WAR", IbSecurityType::Warrant)]
    #[case("BOND", IbSecurityType::Bond)]
    #[case("CMDTY", IbSecurityType::Commodity)]
    #[case("NEWS", IbSecurityType::News)]
    #[case("FUND", IbSecurityType::MutualFund)]
    #[case("CRYPTO", IbSecurityType::Crypto)]
    #[case("CFD", IbSecurityType::Cfd)]
    fn test_ib_security_type_parse(
        #[case] value: &str,
        #[case] expected_security_type: IbSecurityType,
    ) {
        let security_type = IbSecurityType::from_str(value).unwrap();
        assert_eq!(security_type, expected_security_type);
        assert_eq!(security_type.to_string(), value);
        assert_eq!(
            IbSecurityType::try_from(&security_type.ibapi_security_type()).unwrap(),
            expected_security_type
        );
    }

    #[rstest]
    #[case("C", IbOptionRight::Call, OptionKind::Call)]
    #[case("P", IbOptionRight::Put, OptionKind::Put)]
    #[case("CALL", IbOptionRight::Call, OptionKind::Call)]
    #[case("PUT", IbOptionRight::Put, OptionKind::Put)]
    fn test_ib_option_right_parse(
        #[case] value: &str,
        #[case] expected_right: IbOptionRight,
        #[case] expected_option_kind: OptionKind,
    ) {
        let right = IbOptionRight::from_str(value).unwrap();
        assert_eq!(right, expected_right);
        assert_eq!(right.option_kind(), expected_option_kind);
    }

    #[rstest]
    #[case("TRADES", IbHistoricalTickType::Trades)]
    #[case("BID_ASK", IbHistoricalTickType::BidAsk)]
    fn test_ib_historical_tick_type_parse(
        #[case] value: &str,
        #[case] expected_tick_type: IbHistoricalTickType,
    ) {
        let tick_type = IbHistoricalTickType::from_str(value).unwrap();
        assert_eq!(tick_type, expected_tick_type);
        assert_eq!(tick_type.to_string(), value);
    }

    #[rstest]
    #[case(true, IbTradingHours::Regular)]
    #[case(false, IbTradingHours::Extended)]
    fn test_ib_trading_hours_parse(
        #[case] use_rth: bool,
        #[case] expected_trading_hours: IbTradingHours,
    ) {
        let trading_hours = IbTradingHours::from(use_rth);
        assert_eq!(trading_hours, expected_trading_hours);
        assert_eq!(trading_hours.use_rth(), use_rth);
        assert_eq!(
            trading_hours.ibapi_trading_hours().use_rth(),
            expected_trading_hours.use_rth()
        );
    }

    #[rstest]
    #[case(IbHistoricalBarSize::Sec, "1 secs")]
    #[case(IbHistoricalBarSize::Min5, "5 mins")]
    #[case(IbHistoricalBarSize::Hour2, "2 hours")]
    #[case(IbHistoricalBarSize::Day, "1 day")]
    fn test_ib_historical_bar_size_display(
        #[case] bar_size: IbHistoricalBarSize,
        #[case] expected_display: &str,
    ) {
        assert_eq!(bar_size.to_string(), expected_display);
    }

    #[rstest]
    #[case(IbHistoricalWhatToShow::Trades, "TRADES")]
    #[case(IbHistoricalWhatToShow::Midpoint, "MIDPOINT")]
    #[case(IbHistoricalWhatToShow::BidAsk, "BID_ASK")]
    #[case(IbHistoricalWhatToShow::AdjustedLast, "ADJUSTED_LAST")]
    fn test_ib_historical_what_to_show_display(
        #[case] what_to_show: IbHistoricalWhatToShow,
        #[case] expected_display: &str,
    ) {
        assert_eq!(what_to_show.as_str(), expected_display);
        assert_eq!(what_to_show.to_string(), expected_display);
    }

    #[rstest]
    #[case(IbRealtimeBarSize::Sec5, "5 secs")]
    fn test_ib_realtime_bar_size_display(
        #[case] bar_size: IbRealtimeBarSize,
        #[case] expected_display: &str,
    ) {
        assert_eq!(bar_size.to_string(), expected_display);
    }

    #[rstest]
    #[case(IbRealtimeWhatToShow::Trades, "TRADES")]
    #[case(IbRealtimeWhatToShow::Midpoint, "MIDPOINT")]
    #[case(IbRealtimeWhatToShow::Bid, "BID")]
    #[case(IbRealtimeWhatToShow::Ask, "ASK")]
    fn test_ib_realtime_what_to_show_display(
        #[case] what_to_show: IbRealtimeWhatToShow,
        #[case] expected_display: &str,
    ) {
        assert_eq!(what_to_show.as_str(), expected_display);
        assert_eq!(what_to_show.to_string(), expected_display);
    }

    #[rstest]
    #[case("price", IbConditionKind::Price)]
    #[case("time", IbConditionKind::Time)]
    #[case("margin", IbConditionKind::Margin)]
    #[case("execution", IbConditionKind::Execution)]
    #[case("volume", IbConditionKind::Volume)]
    #[case("percent_change", IbConditionKind::PercentChange)]
    fn test_ib_condition_kind_parse(#[case] value: &str, #[case] expected_kind: IbConditionKind) {
        let kind = IbConditionKind::from_str(value).unwrap();
        assert_eq!(kind, expected_kind);
        assert_eq!(kind.to_string(), value);
    }

    #[rstest]
    #[case("and", IbConditionConjunction::And, true)]
    #[case("or", IbConditionConjunction::Or, false)]
    #[case("a", IbConditionConjunction::And, true)]
    #[case("o", IbConditionConjunction::Or, false)]
    fn test_ib_condition_conjunction_parse(
        #[case] value: &str,
        #[case] expected_conjunction: IbConditionConjunction,
        #[case] expected_is_conjunction: bool,
    ) {
        let conjunction = IbConditionConjunction::from_str(value).unwrap();
        assert_eq!(conjunction, expected_conjunction);
        assert_eq!(conjunction.is_conjunction(), expected_is_conjunction);
    }

    #[rstest]
    #[case(0, IbTriggerMethod::Default)]
    #[case(1, IbTriggerMethod::DoubleBidAsk)]
    #[case(2, IbTriggerMethod::Last)]
    #[case(3, IbTriggerMethod::DoubleLast)]
    #[case(4, IbTriggerMethod::BidAsk)]
    #[case(7, IbTriggerMethod::LastOrBidAsk)]
    #[case(8, IbTriggerMethod::Midpoint)]
    fn test_ib_trigger_method_parse(
        #[case] value: i32,
        #[case] expected_trigger_method: IbTriggerMethod,
    ) {
        let trigger_method = IbTriggerMethod::from(value);
        assert_eq!(trigger_method, expected_trigger_method);
        assert_eq!(trigger_method.as_i32(), value);
        assert_eq!(
            IbTriggerMethod::from(trigger_method.ibapi_trigger_method()),
            expected_trigger_method
        );
    }

    #[rstest]
    #[case(0, IbOcaType::None)]
    #[case(1, IbOcaType::CancelWithBlock)]
    #[case(2, IbOcaType::ReduceWithBlock)]
    #[case(3, IbOcaType::ReduceWithoutBlock)]
    fn test_ib_oca_type_parse(#[case] value: i32, #[case] expected_oca_type: IbOcaType) {
        let oca_type = IbOcaType::from(value);
        assert_eq!(oca_type, expected_oca_type);
        assert_eq!(oca_type.as_i32(), value);
        assert_eq!(
            IbOcaType::from(oca_type.ibapi_oca_type()),
            expected_oca_type
        );
    }

    #[rstest]
    #[case(0, IbComboLegOpenClose::Same)]
    #[case(1, IbComboLegOpenClose::Open)]
    #[case(2, IbComboLegOpenClose::Close)]
    #[case(3, IbComboLegOpenClose::Unknown)]
    fn test_ib_combo_leg_open_close_parse(
        #[case] value: i32,
        #[case] expected_open_close: IbComboLegOpenClose,
    ) {
        assert_eq!(expected_open_close.as_i32(), value);
        assert_eq!(
            IbComboLegOpenClose::from(expected_open_close.ibapi_combo_leg_open_close()),
            expected_open_close
        );
    }

    #[rstest]
    #[case(0, IbLiquidity::None)]
    #[case(1, IbLiquidity::AddedLiquidity)]
    #[case(2, IbLiquidity::RemovedLiquidity)]
    #[case(3, IbLiquidity::LiquidityRoutedOut)]
    fn test_ib_liquidity_parse(#[case] value: i32, #[case] expected_liquidity: IbLiquidity) {
        let liquidity = IbLiquidity::from(value);
        assert_eq!(liquidity, expected_liquidity);
        assert_eq!(liquidity.as_i32(), value);
        assert_eq!(
            IbLiquidity::from(liquidity.ibapi_liquidity()),
            expected_liquidity
        );
    }
}
