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

//! URL helpers for Deribit API endpoints.

use super::consts::{
    DERIBIT_HTTP_URL, DERIBIT_TESTNET_HTTP_URL, DERIBIT_TESTNET_WS_URL, DERIBIT_WS_URL,
};

/// Returns the HTTP base URL for the given environment.
#[must_use]
pub fn get_http_base_url(is_testnet: bool) -> &'static str {
    if is_testnet {
        DERIBIT_TESTNET_HTTP_URL
    } else {
        DERIBIT_HTTP_URL
    }
}

/// Returns the WebSocket URL for the given environment.
#[must_use]
pub fn get_ws_url(is_testnet: bool) -> &'static str {
    if is_testnet {
        DERIBIT_TESTNET_WS_URL
    } else {
        DERIBIT_WS_URL
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_http_base_url_production() {
        assert_eq!(get_http_base_url(false), "https://www.deribit.com");
    }

    #[rstest]
    fn test_http_base_url_testnet() {
        assert_eq!(get_http_base_url(true), "https://test.deribit.com");
    }

    #[rstest]
    fn test_ws_url_production() {
        assert_eq!(get_ws_url(false), "wss://www.deribit.com/ws/api/v2");
    }

    #[rstest]
    fn test_ws_url_testnet() {
        assert_eq!(get_ws_url(true), "wss://test.deribit.com/ws/api/v2");
    }
}
