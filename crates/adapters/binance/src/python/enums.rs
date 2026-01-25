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

//! Python bindings for Binance enums.

use pyo3::prelude::*;

use crate::common::enums::{BinanceEnvironment, BinanceProductType};

#[pymethods]
impl BinanceProductType {
    #[classattr]
    #[pyo3(name = "SPOT")]
    const fn py_spot() -> Self {
        Self::Spot
    }

    #[classattr]
    #[pyo3(name = "MARGIN")]
    const fn py_margin() -> Self {
        Self::Margin
    }

    #[classattr]
    #[pyo3(name = "USD_M")]
    const fn py_usd_m() -> Self {
        Self::UsdM
    }

    #[classattr]
    #[pyo3(name = "COIN_M")]
    const fn py_coin_m() -> Self {
        Self::CoinM
    }

    #[classattr]
    #[pyo3(name = "OPTIONS")]
    const fn py_options() -> Self {
        Self::Options
    }

    fn __repr__(&self) -> String {
        format!(
            "BinanceProductType.{}",
            match self {
                Self::Spot => "SPOT",
                Self::Margin => "MARGIN",
                Self::UsdM => "USD_M",
                Self::CoinM => "COIN_M",
                Self::Options => "OPTIONS",
            }
        )
    }

    fn __str__(&self) -> String {
        match self {
            Self::Spot => "SPOT",
            Self::Margin => "MARGIN",
            Self::UsdM => "USD_M",
            Self::CoinM => "COIN_M",
            Self::Options => "OPTIONS",
        }
        .to_string()
    }
}

#[pymethods]
impl BinanceEnvironment {
    #[classattr]
    #[pyo3(name = "MAINNET")]
    const fn py_mainnet() -> Self {
        Self::Mainnet
    }

    #[classattr]
    #[pyo3(name = "TESTNET")]
    const fn py_testnet() -> Self {
        Self::Testnet
    }

    fn __repr__(&self) -> String {
        format!(
            "BinanceEnvironment.{}",
            match self {
                Self::Mainnet => "MAINNET",
                Self::Testnet => "TESTNET",
            }
        )
    }

    fn __str__(&self) -> String {
        match self {
            Self::Mainnet => "MAINNET",
            Self::Testnet => "TESTNET",
        }
        .to_string()
    }
}
