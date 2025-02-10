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

use heck::ToSnakeCase;
use pyo3::prelude::*;

/// Convert the given string from any common case (PascalCase, camelCase, kebab-case, etc.)
/// to *lower* snake_case.
///
/// This function uses the `heck` Rust crate under the hood.
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
pub fn py_convert_to_snake_case(input: &str) -> String {
    input.to_snake_case()
}
