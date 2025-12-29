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

//! Python-idiomatic enum aliases (SCREAMING_SNAKE_CASE).

use pyo3::prelude::*;

use crate::common::enums::{KrakenEnvironment, KrakenProductType};

#[pymethods]
impl KrakenEnvironment {
    #[classattr]
    #[pyo3(name = "MAINNET")]
    fn py_mainnet() -> Self {
        Self::Mainnet
    }

    #[classattr]
    #[pyo3(name = "DEMO")]
    fn py_demo() -> Self {
        Self::Demo
    }
}

#[pymethods]
impl KrakenProductType {
    #[classattr]
    #[pyo3(name = "SPOT")]
    fn py_spot() -> Self {
        Self::Spot
    }

    #[classattr]
    #[pyo3(name = "FUTURES")]
    fn py_futures() -> Self {
        Self::Futures
    }
}
