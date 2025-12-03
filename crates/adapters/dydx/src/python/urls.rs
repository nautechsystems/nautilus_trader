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

//! Python wrapper functions for dYdX URL helpers.

use pyo3::prelude::*;

use crate::common::consts::{
    DYDX_GRPC_URL, DYDX_GRPC_URLS, DYDX_HTTP_URL, DYDX_TESTNET_GRPC_URL, DYDX_TESTNET_GRPC_URLS,
    DYDX_TESTNET_HTTP_URL, DYDX_TESTNET_WS_URL, DYDX_WS_URL,
};

/// Get the gRPC URLs for dYdX based on network.
#[pyfunction]
#[pyo3(name = "get_dydx_grpc_urls")]
#[must_use]
pub fn py_get_dydx_grpc_urls(is_testnet: bool) -> Vec<String> {
    if is_testnet {
        DYDX_TESTNET_GRPC_URLS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        DYDX_GRPC_URLS.iter().map(|s| (*s).to_string()).collect()
    }
}

/// Get the primary gRPC URL for dYdX based on network.
#[pyfunction]
#[pyo3(name = "get_dydx_grpc_url")]
#[must_use]
pub fn py_get_dydx_grpc_url(is_testnet: bool) -> String {
    if is_testnet {
        DYDX_TESTNET_GRPC_URL.to_string()
    } else {
        DYDX_GRPC_URL.to_string()
    }
}

/// Get the HTTP base URL for dYdX based on network.
#[pyfunction]
#[pyo3(name = "get_dydx_http_url")]
#[must_use]
pub fn py_get_dydx_http_url(is_testnet: bool) -> String {
    if is_testnet {
        DYDX_TESTNET_HTTP_URL.to_string()
    } else {
        DYDX_HTTP_URL.to_string()
    }
}

/// Get the WebSocket URL for dYdX based on network.
#[pyfunction]
#[pyo3(name = "get_dydx_ws_url")]
#[must_use]
pub fn py_get_dydx_ws_url(is_testnet: bool) -> String {
    if is_testnet {
        DYDX_TESTNET_WS_URL.to_string()
    } else {
        DYDX_WS_URL.to_string()
    }
}
