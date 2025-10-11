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

//! Python bindings for Hyperliquid URL helper functions.

use pyo3::prelude::*;

use crate::common::consts::{info_url, ws_url};

/// Get the HTTP base URL for Hyperliquid API (info endpoint).
///
/// # Arguments
///
/// * `is_testnet` - Whether to use the testnet URL.
///
/// # Returns
///
/// The HTTP base URL string.
#[pyfunction]
#[pyo3(name = "get_hyperliquid_http_base_url")]
pub fn get_hyperliquid_http_base_url(is_testnet: bool) -> String {
    info_url(is_testnet).to_string()
}

/// Get the WebSocket URL for Hyperliquid API.
///
/// # Arguments
///
/// * `is_testnet` - Whether to use the testnet URL.
///
/// # Returns
///
/// The WebSocket URL string.
#[pyfunction]
#[pyo3(name = "get_hyperliquid_ws_url")]
pub fn get_hyperliquid_ws_url(is_testnet: bool) -> String {
    ws_url(is_testnet).to_string()
}
