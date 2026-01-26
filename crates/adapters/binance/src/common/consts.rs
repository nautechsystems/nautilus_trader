// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{num::NonZeroU32, sync::LazyLock};

use nautilus_model::identifiers::Venue;
use nautilus_network::ratelimiter::quota::Quota;
use ustr::Ustr;

use super::enums::{BinanceRateLimitInterval, BinanceRateLimitType};

/// The Binance venue identifier string.
pub const BINANCE: &str = "BINANCE";

/// Static venue instance for Binance.
pub static BINANCE_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(BINANCE));

/// Binance Spot API base URL (mainnet).
pub const BINANCE_SPOT_HTTP_URL: &str = "https://api.binance.com";

/// Binance USD-M Futures API base URL (mainnet).
pub const BINANCE_FUTURES_USD_HTTP_URL: &str = "https://fapi.binance.com";

/// Binance COIN-M Futures API base URL (mainnet).
pub const BINANCE_FUTURES_COIN_HTTP_URL: &str = "https://dapi.binance.com";

/// Binance European Options API base URL (mainnet).
pub const BINANCE_OPTIONS_HTTP_URL: &str = "https://eapi.binance.com";

/// Binance Spot API base URL (testnet).
pub const BINANCE_SPOT_TESTNET_HTTP_URL: &str = "https://testnet.binance.vision";

/// Binance USD-M Futures API base URL (testnet).
pub const BINANCE_FUTURES_USD_TESTNET_HTTP_URL: &str = "https://testnet.binancefuture.com";

/// Binance COIN-M Futures API base URL (testnet).
pub const BINANCE_FUTURES_COIN_TESTNET_HTTP_URL: &str = "https://testnet.binancefuture.com";

/// Binance Spot WebSocket base URL (mainnet).
pub const BINANCE_SPOT_WS_URL: &str = "wss://stream.binance.com:9443/ws";

/// Binance USD-M Futures WebSocket base URL (mainnet).
pub const BINANCE_FUTURES_USD_WS_URL: &str = "wss://fstream.binance.com/ws";

/// Binance COIN-M Futures WebSocket base URL (mainnet).
pub const BINANCE_FUTURES_COIN_WS_URL: &str = "wss://dstream.binance.com/ws";

/// Binance European Options WebSocket base URL (mainnet).
pub const BINANCE_OPTIONS_WS_URL: &str = "wss://nbstream.binance.com/eoptions";

/// Binance Spot SBE WebSocket stream URL (mainnet).
pub const BINANCE_SPOT_SBE_WS_URL: &str = "wss://stream-sbe.binance.com/ws";

/// Binance Spot SBE WebSocket API URL (mainnet).
pub const BINANCE_SPOT_SBE_WS_API_URL: &str =
    "wss://ws-api.binance.com:443/ws-api/v3?responseFormat=sbe&sbeSchemaId=3&sbeSchemaVersion=2";

/// Binance Spot SBE WebSocket API URL (testnet).
pub const BINANCE_SPOT_SBE_WS_API_TESTNET_URL: &str =
    "wss://testnet.binance.vision/ws-api/v3?responseFormat=sbe&sbeSchemaId=3&sbeSchemaVersion=2";

/// Binance Spot WebSocket base URL (testnet).
pub const BINANCE_SPOT_TESTNET_WS_URL: &str = "wss://testnet.binance.vision/ws";

/// Binance USD-M Futures WebSocket base URL (testnet).
pub const BINANCE_FUTURES_USD_TESTNET_WS_URL: &str = "wss://stream.binancefuture.com/ws";

/// Binance COIN-M Futures WebSocket base URL (testnet).
pub const BINANCE_FUTURES_COIN_TESTNET_WS_URL: &str = "wss://dstream.binancefuture.com/ws";

/// Binance Spot API version path.
pub const BINANCE_SPOT_API_PATH: &str = "/api/v3";

/// Binance USD-M Futures API version path.
pub const BINANCE_FAPI_PATH: &str = "/fapi/v1";

/// Binance COIN-M Futures API version path.
pub const BINANCE_DAPI_PATH: &str = "/dapi/v1";

/// Binance European Options API version path.
pub const BINANCE_EAPI_PATH: &str = "/eapi/v1";

/// Describes a static rate limit quota for a product type.
#[derive(Clone, Copy, Debug)]
pub struct BinanceRateLimitQuota {
    /// Rate limit type.
    pub rate_limit_type: BinanceRateLimitType,
    /// Time interval unit.
    pub interval: BinanceRateLimitInterval,
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
        rate_limit_type: BinanceRateLimitType::RequestWeight,
        interval: BinanceRateLimitInterval::Minute,
        interval_num: 1,
        limit: 1_200,
    },
    BinanceRateLimitQuota {
        rate_limit_type: BinanceRateLimitType::Orders,
        interval: BinanceRateLimitInterval::Second,
        interval_num: 1,
        limit: 10,
    },
    BinanceRateLimitQuota {
        rate_limit_type: BinanceRateLimitType::Orders,
        interval: BinanceRateLimitInterval::Day,
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
        rate_limit_type: BinanceRateLimitType::RequestWeight,
        interval: BinanceRateLimitInterval::Minute,
        interval_num: 1,
        limit: 2_400,
    },
    BinanceRateLimitQuota {
        rate_limit_type: BinanceRateLimitType::Orders,
        interval: BinanceRateLimitInterval::Second,
        interval_num: 1,
        limit: 50,
    },
    BinanceRateLimitQuota {
        rate_limit_type: BinanceRateLimitType::Orders,
        interval: BinanceRateLimitInterval::Minute,
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
        rate_limit_type: BinanceRateLimitType::RequestWeight,
        interval: BinanceRateLimitInterval::Minute,
        interval_num: 1,
        limit: 1_200,
    },
    BinanceRateLimitQuota {
        rate_limit_type: BinanceRateLimitType::Orders,
        interval: BinanceRateLimitInterval::Second,
        interval_num: 1,
        limit: 20,
    },
    BinanceRateLimitQuota {
        rate_limit_type: BinanceRateLimitType::Orders,
        interval: BinanceRateLimitInterval::Minute,
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
        rate_limit_type: BinanceRateLimitType::RequestWeight,
        interval: BinanceRateLimitInterval::Minute,
        interval_num: 1,
        limit: 3_000,
    },
    BinanceRateLimitQuota {
        rate_limit_type: BinanceRateLimitType::Orders,
        interval: BinanceRateLimitInterval::Second,
        interval_num: 1,
        limit: 5,
    },
    BinanceRateLimitQuota {
        rate_limit_type: BinanceRateLimitType::Orders,
        interval: BinanceRateLimitInterval::Minute,
        interval_num: 1,
        limit: 200,
    },
];

/// WebSocket subscription rate limit: 5 messages per second.
///
/// Binance limits incoming WebSocket messages (subscribe/unsubscribe) to 5 per second.
pub static BINANCE_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(5).expect("5 > 0")));

/// WebSocket connection rate limit: 1 per second (conservative).
///
/// Binance limits connections to 300 per 5 minutes per IP. This conservative quota
/// of 1 per second helps avoid hitting the connection limit during reconnection storms.
pub static BINANCE_WS_CONNECTION_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(1).expect("1 > 0")));

/// Pre-interned rate limit key for WebSocket subscription operations.
pub static BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from("subscription")]);

/// Valid order book depth levels for Binance.
pub const BINANCE_BOOK_DEPTHS: [u32; 7] = [5, 10, 20, 50, 100, 500, 1000];
