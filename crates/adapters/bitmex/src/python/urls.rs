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

//! Python wrapper functions for BitMEX URL helpers.

use pyo3::prelude::*;

use crate::common::urls;

/// Gets the BitMEX HTTP base URL.
#[pyfunction]
pub fn get_bitmex_http_base_url(testnet: bool) -> String {
    urls::get_http_base_url(testnet)
}

/// Gets the BitMEX WebSocket URL.
#[pyfunction]
pub fn get_bitmex_ws_url(testnet: bool) -> String {
    urls::get_ws_url(testnet)
}
