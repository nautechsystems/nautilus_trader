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

#![allow(clippy::doc_markdown, reason = "Python docstrings")]

use pyo3::prelude::*;
use pyo3_stub_gen::derive::gen_stub_pyfunction;

/// Masks an API key by showing only the first and last 4 characters.
///
/// For keys 8 characters or shorter, returns asterisks only.
///
/// Parameters
/// ----------
/// api_key : str
///     The API key to mask.
///
/// Returns
/// -------
/// str
///
/// Examples
/// --------
/// >>> mask_api_key("abcdefghijklmnop")
/// 'abcd...mnop'
/// >>> mask_api_key("short")
/// '*****'
///
#[gen_stub_pyfunction(module = "nautilus_trader.core")]
#[pyfunction(name = "mask_api_key")]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Python FFI requires owned types"
)]
pub fn py_mask_api_key(api_key: String) -> String {
    crate::string::mask_api_key(&api_key)
}
