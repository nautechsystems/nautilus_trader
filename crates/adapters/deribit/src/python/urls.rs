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

//! Python URL helper functions for Deribit.

use pyo3::prelude::*;

use crate::common::consts::{
    DERIBIT_HTTP_URL, DERIBIT_TESTNET_HTTP_URL, DERIBIT_TESTNET_WS_URL, DERIBIT_WS_URL,
};

/// Returns the Deribit HTTP base URL.
///
/// # Arguments
///
/// * `is_testnet` - If true, returns the testnet URL.
#[pyfunction]
#[pyo3(name = "get_deribit_http_base_url")]
#[must_use]
pub fn py_get_deribit_http_base_url(is_testnet: bool) -> String {
    if is_testnet {
        DERIBIT_TESTNET_HTTP_URL.to_string()
    } else {
        DERIBIT_HTTP_URL.to_string()
    }
}

/// Returns the Deribit WebSocket URL.
///
/// # Arguments
///
/// * `is_testnet` - If true, returns the testnet URL.
#[pyfunction]
#[pyo3(name = "get_deribit_ws_url")]
#[must_use]
pub fn py_get_deribit_ws_url(is_testnet: bool) -> String {
    if is_testnet {
        DERIBIT_TESTNET_WS_URL.to_string()
    } else {
        DERIBIT_WS_URL.to_string()
    }
}
