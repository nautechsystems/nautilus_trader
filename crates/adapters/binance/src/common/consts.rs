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

//! Binance venue constants and API endpoints.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;

/// The Binance venue identifier string.
pub const BINANCE: &str = "BINANCE";

/// Static venue instance for Binance.
pub static BINANCE_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(BINANCE));

// ------------------------------------------------------------------------------------------------
// HTTP Base URLs - Mainnet
// ------------------------------------------------------------------------------------------------

/// Binance Spot API base URL (mainnet).
pub const BINANCE_SPOT_HTTP_URL: &str = "https://api.binance.com";

/// Binance USD-M Futures API base URL (mainnet).
pub const BINANCE_FUTURES_USD_HTTP_URL: &str = "https://fapi.binance.com";

/// Binance COIN-M Futures API base URL (mainnet).
pub const BINANCE_FUTURES_COIN_HTTP_URL: &str = "https://dapi.binance.com";

/// Binance European Options API base URL (mainnet).
pub const BINANCE_OPTIONS_HTTP_URL: &str = "https://eapi.binance.com";

// ------------------------------------------------------------------------------------------------
// HTTP Base URLs - Testnet
// ------------------------------------------------------------------------------------------------

/// Binance Spot API base URL (testnet).
pub const BINANCE_SPOT_TESTNET_HTTP_URL: &str = "https://testnet.binance.vision";

/// Binance USD-M Futures API base URL (testnet).
pub const BINANCE_FUTURES_USD_TESTNET_HTTP_URL: &str = "https://testnet.binancefuture.com";

/// Binance COIN-M Futures API base URL (testnet).
pub const BINANCE_FUTURES_COIN_TESTNET_HTTP_URL: &str = "https://testnet.binancefuture.com";

// Note: Binance Options testnet is not publicly available

// ------------------------------------------------------------------------------------------------
// WebSocket URLs - Mainnet
// ------------------------------------------------------------------------------------------------

/// Binance Spot WebSocket base URL (mainnet).
pub const BINANCE_SPOT_WS_URL: &str = "wss://stream.binance.com:9443/ws";

/// Binance USD-M Futures WebSocket base URL (mainnet).
pub const BINANCE_FUTURES_USD_WS_URL: &str = "wss://fstream.binance.com/ws";

/// Binance COIN-M Futures WebSocket base URL (mainnet).
pub const BINANCE_FUTURES_COIN_WS_URL: &str = "wss://dstream.binance.com/ws";

/// Binance European Options WebSocket base URL (mainnet).
pub const BINANCE_OPTIONS_WS_URL: &str = "wss://nbstream.binance.com/eoptions";

// ------------------------------------------------------------------------------------------------
// WebSocket URLs - Testnet
// ------------------------------------------------------------------------------------------------

/// Binance Spot WebSocket base URL (testnet).
pub const BINANCE_SPOT_TESTNET_WS_URL: &str = "wss://testnet.binance.vision/ws";

/// Binance USD-M Futures WebSocket base URL (testnet).
pub const BINANCE_FUTURES_USD_TESTNET_WS_URL: &str = "wss://stream.binancefuture.com/ws";

/// Binance COIN-M Futures WebSocket base URL (testnet).
pub const BINANCE_FUTURES_COIN_TESTNET_WS_URL: &str = "wss://dstream.binancefuture.com/ws";

// ------------------------------------------------------------------------------------------------
// API Paths
// ------------------------------------------------------------------------------------------------

/// Binance Spot API version path.
pub const BINANCE_SPOT_API_PATH: &str = "/api/v3";

/// Binance USD-M Futures API version path.
pub const BINANCE_FAPI_PATH: &str = "/fapi/v1";

/// Binance COIN-M Futures API version path.
pub const BINANCE_DAPI_PATH: &str = "/dapi/v1";

/// Binance European Options API version path.
pub const BINANCE_EAPI_PATH: &str = "/eapi/v1";

// ------------------------------------------------------------------------------------------------
// Rate Limiting
// ------------------------------------------------------------------------------------------------

/// Describes a static rate limit quota for a product type.
#[derive(Clone, Copy, Debug)]
pub struct BinanceRateLimitQuota {
    /// Rate limit type identifier (REQUEST_WEIGHT or ORDERS).
    pub rate_limit_type: &'static str,
    /// Time interval unit (SECOND, MINUTE, DAY).
    pub interval: &'static str,
    /// Number of intervals.
    pub interval_num: u32,
    /// Maximum allowed requests for the interval.
    pub limit: u32,
}

/// Spot & margin REST limits (default IP weights).
///
/// References:
/// - <https://developers.binance.com/docs/binance-spot-api-docs/limits>
pub const BINANCE_SPOT_RATE_LIMITS: &[BinanceRateLimitQuota] = &[
    BinanceRateLimitQuota {
        rate_limit_type: "REQUEST_WEIGHT",
        interval: "MINUTE",
        interval_num: 1,
        limit: 1_200,
    },
    BinanceRateLimitQuota {
        rate_limit_type: "ORDERS",
        interval: "SECOND",
        interval_num: 1,
        limit: 10,
    },
    BinanceRateLimitQuota {
        rate_limit_type: "ORDERS",
        interval: "DAY",
        interval_num: 1,
        limit: 100_000,
    },
];

/// USD-M Futures REST limits (default IP weights).
///
/// References:
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/general-info#limits>
pub const BINANCE_FAPI_RATE_LIMITS: &[BinanceRateLimitQuota] = &[
    BinanceRateLimitQuota {
        rate_limit_type: "REQUEST_WEIGHT",
        interval: "MINUTE",
        interval_num: 1,
        limit: 2_400,
    },
    BinanceRateLimitQuota {
        rate_limit_type: "ORDERS",
        interval: "SECOND",
        interval_num: 1,
        limit: 50,
    },
    BinanceRateLimitQuota {
        rate_limit_type: "ORDERS",
        interval: "MINUTE",
        interval_num: 1,
        limit: 1_200,
    },
];

/// COIN-M Futures REST limits (default IP weights).
///
/// References:
/// - <https://developers.binance.com/docs/derivatives/coin-margined-futures/general-info#limits>
pub const BINANCE_DAPI_RATE_LIMITS: &[BinanceRateLimitQuota] = &[
    BinanceRateLimitQuota {
        rate_limit_type: "REQUEST_WEIGHT",
        interval: "MINUTE",
        interval_num: 1,
        limit: 1_200,
    },
    BinanceRateLimitQuota {
        rate_limit_type: "ORDERS",
        interval: "SECOND",
        interval_num: 1,
        limit: 20,
    },
    BinanceRateLimitQuota {
        rate_limit_type: "ORDERS",
        interval: "MINUTE",
        interval_num: 1,
        limit: 1_200,
    },
];

/// Options REST limits (default IP weights).
///
/// References:
/// - <https://developers.binance.com/docs/derivatives/european-options/general-info#limits>
pub const BINANCE_EAPI_RATE_LIMITS: &[BinanceRateLimitQuota] = &[
    BinanceRateLimitQuota {
        rate_limit_type: "REQUEST_WEIGHT",
        interval: "MINUTE",
        interval_num: 1,
        limit: 3_000,
    },
    BinanceRateLimitQuota {
        rate_limit_type: "ORDERS",
        interval: "SECOND",
        interval_num: 1,
        limit: 5,
    },
    BinanceRateLimitQuota {
        rate_limit_type: "ORDERS",
        interval: "MINUTE",
        interval_num: 1,
        limit: 200,
    },
];
