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

use ::pyo3::exceptions::PyValueError;
use ::strum::{IntoEnumIterator, ParseError};
use pyo3::PyResult;

/// Converts a raw string to the enum `E`, returning a nicely‑formatted
/// `PyValueError` if the string does not match any variant.
///
/// The helper is aimed at Python‑exposed functions that still accept plain
/// `&str` parameters internally: call `parse_enum` instead of writing repetitive
/// `str::parse()` + error‑formatting logic yourself.
///
/// # Errors
///
/// Returns an error if `input` does not match any known variant of `E`.
pub fn parse_enum<E>(input: &str, param: &str) -> PyResult<E>
where
    E: std::str::FromStr<Err = ParseError> + IntoEnumIterator + ToString,
{
    input.parse::<E>().map_err(|_| {
        let allowed = E::iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        PyValueError::new_err(format!(
            "unknown {param} `{input}`; valid values: {allowed}"
        ))
    })
}
