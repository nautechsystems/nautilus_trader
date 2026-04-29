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

//! String-case conversion helpers (`CamelCase` ⇄ `snake_case`).

use pyo3::prelude::*;
use pyo3_stub_gen::derive::gen_stub_pyfunction;

use crate::string::conversions::to_snake_case;

/// Convert the given string from any common case (PascalCase, camelCase, kebab-case, etc.)
/// to *lower* `snake_case`.
///
/// Parameters
/// ----------
/// input : str
///     The input string to convert.
///
/// Returns
/// -------
/// str
#[must_use]
#[pyfunction(name = "convert_to_snake_case")]
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
pub fn py_convert_to_snake_case(input: &str) -> String {
    to_snake_case(input)
}
