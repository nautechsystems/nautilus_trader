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

use strum::Display;

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Eq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleBodySize {
    None = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
    Trend = 4,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Eq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleDirection {
    Bull = 1,
    None = 0,
    Bear = -1,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Eq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleSize {
    None = 0,
    VerySmall = 1,
    Small = 2,
    Medium = 3,
    Large = 4,
    VeryLarge = 5,
    ExtremelyLarge = 6,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Eq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleWickSize {
    None = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
}
