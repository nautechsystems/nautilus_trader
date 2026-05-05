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

use std::{fmt::Display, str::FromStr};

use nautilus_model::enums::{
    OrderSide, OrderStatus as NautilusOrderStatus, OrderType as NautilusOrderType,
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
            Self::MarketWithProtection
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
            | Self::RelativeMarketCombo => NautilusOrderType::Market,
            Self::RelativeLimitCombo => NautilusOrderType::Limit,
            Self::StopWithProtection => NautilusOrderType::StopMarket,
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
            "PEG MID" | "PEGMID" => Ok(Self::PeggedToMidpoint),
            "PEG BENCH" | "PEGBENCH" => Ok(Self::PeggedToBenchmark),
            "PEG BEST" | "PEGBEST" => Ok(Self::PegBest),
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
