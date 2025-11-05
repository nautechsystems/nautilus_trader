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

//! URL helpers for dYdX services.

use super::consts::{
    DYDX_GRPC_URLS, DYDX_HTTP_URL, DYDX_TESTNET_GRPC_URLS, DYDX_TESTNET_HTTP_URL,
    DYDX_TESTNET_WS_URL, DYDX_WS_URL,
};

/// Gets the HTTP base URL for the specified network.
#[must_use]
pub const fn http_base_url(is_testnet: bool) -> &'static str {
    if is_testnet {
        DYDX_TESTNET_HTTP_URL
    } else {
        DYDX_HTTP_URL
    }
}

/// Gets the WebSocket URL for the specified network.
#[must_use]
pub const fn ws_url(is_testnet: bool) -> &'static str {
    if is_testnet {
        DYDX_TESTNET_WS_URL
    } else {
        DYDX_WS_URL
    }
}

/// Gets the gRPC URLs with fallback support for the specified network.
///
/// Returns an array of gRPC endpoints that should be tried in order.
/// This is important for DEX environments where individual validator nodes
/// can become unavailable or fail.
#[must_use]
pub const fn grpc_urls(is_testnet: bool) -> &'static [&'static str] {
    if is_testnet {
        DYDX_TESTNET_GRPC_URLS
    } else {
        DYDX_GRPC_URLS
    }
}

/// Gets the primary gRPC URL for the specified network.
///
/// # Notes
///
/// For production use, consider using `grpc_urls()` to get all available
/// endpoints and implement fallback logic via `DydxGrpcClient::new_with_fallback()`.
#[must_use]
pub const fn grpc_url(is_testnet: bool) -> &'static str {
    grpc_urls(is_testnet)[0]
}
