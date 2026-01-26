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

//! Common enumerations for the Deribit adapter.

use std::fmt::Display;

use nautilus_model::enums::TimeInForce;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display as StrumDisplay, EnumIter, EnumString};

/// Deribit instrument kind/type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    StrumDisplay,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub enum DeribitInstrumentKind {
    /// Future contract
    Future,
    /// Option contract
    Option,
    /// Spot market
    Spot,
    /// Future combo
    #[serde(rename = "future_combo")]
    FutureCombo,
    /// Option combo
    #[serde(rename = "option_combo")]
    OptionCombo,
}

/// Deribit currency.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub enum DeribitCurrency {
    /// Bitcoin
    BTC,
    /// Ethereum
    ETH,
    /// USD Coin
    USDC,
    /// Tether
    USDT,
    /// Euro stablecoin
    EURR,
    /// All currencies
    #[serde(rename = "any")]
    ANY,
}

impl DeribitCurrency {
    /// Returns the currency as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BTC => "BTC",
            Self::ETH => "ETH",
            Self::USDC => "USDC",
            Self::USDT => "USDT",
            Self::EURR => "EURR",
            Self::ANY => "any",
        }
    }
}

impl Display for DeribitCurrency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Deribit option type.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    StrumDisplay,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeribitOptionType {
    /// Call option
    Call,
    /// Put option
    Put,
}

/// Deribit instrument lifecycle state.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeribitInstrumentState {
    /// Instrument has been created but not yet active.
    Created,
    /// Instrument is active and trading.
    Started,
    /// Instrument has been settled (options/futures at expiry).
    Settled,
    /// Instrument is closed for trading.
    Closed,
    /// Instrument has been terminated.
    Terminated,
}

impl Display for DeribitInstrumentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Started => write!(f, "started"),
            Self::Settled => write!(f, "settled"),
            Self::Closed => write!(f, "closed"),
            Self::Terminated => write!(f, "terminated"),
        }
    }
}

/// Deribit time in force values for order execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeribitTimeInForce {
    /// Good till cancelled.
    #[serde(rename = "good_til_cancelled")]
    GoodTilCancelled,
    /// Good till day (expires at end of trading day).
    #[serde(rename = "good_til_day")]
    GoodTilDay,
    /// Immediate or cancel.
    #[serde(rename = "immediate_or_cancel")]
    ImmediateOrCancel,
    /// Fill or kill.
    #[serde(rename = "fill_or_kill")]
    FillOrKill,
}

impl DeribitTimeInForce {
    /// Returns the time in force as a Deribit API string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GoodTilCancelled => "good_til_cancelled",
            Self::GoodTilDay => "good_til_day",
            Self::ImmediateOrCancel => "immediate_or_cancel",
            Self::FillOrKill => "fill_or_kill",
        }
    }
}

impl TryFrom<TimeInForce> for DeribitTimeInForce {
    type Error = String;

    fn try_from(tif: TimeInForce) -> Result<Self, Self::Error> {
        match tif {
            TimeInForce::Gtc => Ok(Self::GoodTilCancelled),
            TimeInForce::Ioc => Ok(Self::ImmediateOrCancel),
            TimeInForce::Fok => Ok(Self::FillOrKill),
            TimeInForce::Gtd => Ok(Self::GoodTilDay),
            _ => Err(format!(
                "TimeInForce::{tif} is not supported on Deribit (valid: GTC, IOC, FOK, GTD)"
            )),
        }
    }
}
