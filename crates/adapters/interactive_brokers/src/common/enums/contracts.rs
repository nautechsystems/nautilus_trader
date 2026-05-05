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

use nautilus_model::enums::OptionKind;

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
