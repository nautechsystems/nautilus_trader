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

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use reqwest::StatusCode;
use ustr::Ustr;

/// dYdX adapter name.
pub const DYDX: &str = "DYDX";

/// dYdX venue identifier.
pub static DYDX_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(DYDX)));

/// dYdX mainnet chain ID.
pub const DYDX_CHAIN_ID: &str = "dydx-mainnet-1";

/// dYdX testnet chain ID.
pub const DYDX_TESTNET_CHAIN_ID: &str = "dydx-testnet-4";

/// Cosmos SDK bech32 address prefix for dYdX.
pub const DYDX_BECH32_PREFIX: &str = "dydx";

/// USDC gas denomination (native chain token).
pub const USDC_GAS_DENOM: &str =
    "ibc/8E27BA2D5493AF5636760E354E46004562C46AB7EC0CC4C1CA14E9E20E2545B5";

/// USDC asset denomination for transfers.
pub const USDC_DENOM: &str = "uusdc";

/// HD wallet derivation path for dYdX accounts (Cosmos SLIP-0044).
/// Format: m/44'/118'/0'/0/{account_index}
pub const DYDX_DERIVATION_PATH_PREFIX: &str = "m/44'/118'/0'/0";

/// Coin type for Cosmos ecosystem (SLIP-0044).
pub const COSMOS_COIN_TYPE: u32 = 118;

// Mainnet URLs
/// dYdX v4 mainnet HTTP API base URL.
pub const DYDX_HTTP_URL: &str = "https://indexer.dydx.trade";

/// dYdX v4 mainnet WebSocket URL.
pub const DYDX_WS_URL: &str = "wss://indexer.dydx.trade/v4/ws";

/// dYdX v4 mainnet gRPC URLs (public validator nodes with fallbacks).
///
/// Multiple nodes are provided for redundancy. The client should attempt to connect
/// to nodes in order, falling back to the next if connection fails. This is critical
/// for DEX environments where individual nodes can fail or become unavailable.
///
/// Endpoints sourced from:
/// - https://docs.dydx.xyz/interaction/endpoints#node
///
/// # Notes
///
/// URLs use domain:port format for tonic gRPC client (TLS is automatic on port 443).
pub const DYDX_GRPC_URLS: &[&str] = &[
    "https://dydx-ops-grpc.kingnodes.com:443",
    "https://dydx-dao-grpc-1.polkachu.com:443",
    "https://dydx-grpc.publicnode.com:443",
];

/// dYdX v4 mainnet gRPC URL (primary public node).
///
/// # Notes
///
/// For production use, consider using `DYDX_GRPC_URLS` array with fallback logic
/// via `DydxGrpcClient::new_with_fallback()`.
pub const DYDX_GRPC_URL: &str = DYDX_GRPC_URLS[0];

// Testnet URLs
/// dYdX v4 testnet HTTP API base URL.
pub const DYDX_TESTNET_HTTP_URL: &str = "https://indexer.v4testnet.dydx.exchange";

/// dYdX v4 testnet WebSocket URL.
pub const DYDX_TESTNET_WS_URL: &str = "wss://indexer.v4testnet.dydx.exchange/v4/ws";

/// dYdX v4 testnet gRPC URLs (public validator nodes with fallbacks).
///
/// Multiple nodes are provided for redundancy. The client should attempt to connect
/// to nodes in order, falling back to the next if connection fails.
///
/// Endpoints sourced from:
/// - https://docs.dydx.xyz/interaction/endpoints#node
///
/// # Notes
///
/// URLs use domain:port format for tonic gRPC client (TLS is automatic on port 443).
pub const DYDX_TESTNET_GRPC_URLS: &[&str] = &[
    "https://test-dydx-grpc.kingnodes.com:443",
    "https://testnet-dydx.lavenderfive.com:443",
];

/// dYdX v4 testnet gRPC URL (primary public node).
///
/// # Notes
///
/// For production use, consider using `DYDX_TESTNET_GRPC_URLS` array with fallback logic
/// via `DydxGrpcClient::new_with_fallback()`.
pub const DYDX_TESTNET_GRPC_URL: &str = DYDX_TESTNET_GRPC_URLS[0];

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
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_should_retry_429() {
        assert!(should_retry_error_code(&StatusCode::TOO_MANY_REQUESTS));
    }

    #[rstest]
    fn test_should_retry_server_errors() {
        assert!(should_retry_error_code(&StatusCode::INTERNAL_SERVER_ERROR));
        assert!(should_retry_error_code(&StatusCode::BAD_GATEWAY));
        assert!(should_retry_error_code(&StatusCode::SERVICE_UNAVAILABLE));
        assert!(should_retry_error_code(&StatusCode::GATEWAY_TIMEOUT));
    }

    #[rstest]
    fn test_should_not_retry_client_errors() {
        assert!(!should_retry_error_code(&StatusCode::BAD_REQUEST));
        assert!(!should_retry_error_code(&StatusCode::UNAUTHORIZED));
        assert!(!should_retry_error_code(&StatusCode::FORBIDDEN));
        assert!(!should_retry_error_code(&StatusCode::NOT_FOUND));
    }

    #[rstest]
    fn test_should_not_retry_success() {
        assert!(!should_retry_error_code(&StatusCode::OK));
        assert!(!should_retry_error_code(&StatusCode::CREATED));
    }
}
