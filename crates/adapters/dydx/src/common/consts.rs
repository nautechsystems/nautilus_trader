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

//! Core constants shared across the dYdX adapter components.

use reqwest::StatusCode;

/// dYdX adapter name.
pub const DYDX: &str = "DYDX";

/// dYdX mainnet chain ID.
pub const DYDX_CHAIN_ID: &str = "dydx-mainnet-1";

/// dYdX testnet chain ID.
pub const DYDX_TESTNET_CHAIN_ID: &str = "dydx-testnet-4";

/// Determines if an HTTP status code should trigger a retry.
///
/// Retries on:
/// - 429 (Too Many Requests)
/// - 500-599 (Server Errors)
///
/// Does NOT retry on:
/// - 400 (Bad Request) - indicates client error that won't be fixed by retrying
/// - 401 (Unauthorized) - not applicable for dYdX Indexer (no auth required)
/// - 403 (Forbidden) - typically compliance/screening issues
/// - 404 (Not Found) - resource doesn't exist
#[must_use]
pub const fn should_retry_error_code(status: &StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 500..=599)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_retry_429() {
        assert!(should_retry_error_code(&StatusCode::TOO_MANY_REQUESTS));
    }

    #[test]
    fn test_should_retry_server_errors() {
        assert!(should_retry_error_code(&StatusCode::INTERNAL_SERVER_ERROR));
        assert!(should_retry_error_code(&StatusCode::BAD_GATEWAY));
        assert!(should_retry_error_code(&StatusCode::SERVICE_UNAVAILABLE));
        assert!(should_retry_error_code(&StatusCode::GATEWAY_TIMEOUT));
    }

    #[test]
    fn test_should_not_retry_client_errors() {
        assert!(!should_retry_error_code(&StatusCode::BAD_REQUEST));
        assert!(!should_retry_error_code(&StatusCode::UNAUTHORIZED));
        assert!(!should_retry_error_code(&StatusCode::FORBIDDEN));
        assert!(!should_retry_error_code(&StatusCode::NOT_FOUND));
    }

    #[test]
    fn test_should_not_retry_success() {
        assert!(!should_retry_error_code(&StatusCode::OK));
        assert!(!should_retry_error_code(&StatusCode::CREATED));
    }
}
