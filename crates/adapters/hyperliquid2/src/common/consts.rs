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

use std::num::NonZeroU32;
use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use nautilus_network::ratelimiter::quota::Quota;
use ustr::Ustr;

pub const HYPERLIQUID: &str = "HYPERLIQUID";
pub static HYPERLIQUID_VENUE: LazyLock<Venue> =
    LazyLock::new(|| Venue::new(Ustr::from(HYPERLIQUID)));

// Hyperliquid API URLs
pub const HYPERLIQUID_HTTP_URL: &str = "https://api.hyperliquid.xyz";
pub const HYPERLIQUID_TESTNET_HTTP_URL: &str = "https://api.hyperliquid-testnet.xyz";
pub const HYPERLIQUID_WS_URL: &str = "wss://api.hyperliquid.xyz/ws";
pub const HYPERLIQUID_TESTNET_WS_URL: &str = "wss://api.hyperliquid-testnet.xyz/ws";

// API endpoints
pub const HYPERLIQUID_INFO_ENDPOINT: &str = "/info";
pub const HYPERLIQUID_EXCHANGE_ENDPOINT: &str = "/exchange";

// Rate limits (based on Hyperliquid documentation)
/// Hyperliquid HTTP rate limit: 1200 requests per minute
pub static HYPERLIQUID_HTTP_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_minute(NonZeroU32::new(1200).unwrap())
});

/// Hyperliquid WebSocket rate limit: No explicit limit, but connection-based
pub static HYPERLIQUID_WS_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(100).unwrap())
});

// Trading constants
pub const HYPERLIQUID_CLIENT_ID: &str = "HYPERLIQUID";

// Authentication constants
pub const HYPERLIQUID_SIGNATURE_VERSION: &str = "1";
pub const HYPERLIQUID_API_WALLET_ENV_KEY: &str = "HYPERLIQUID_WALLET_ADDRESS";
pub const HYPERLIQUID_PRIVATE_KEY_ENV_KEY: &str = "HYPERLIQUID_PRIVATE_KEY";
pub const HYPERLIQUID_TESTNET_WALLET_ENV_KEY: &str = "HYPERLIQUID_TESTNET_WALLET_ADDRESS";  
pub const HYPERLIQUID_TESTNET_PRIVATE_KEY_ENV_KEY: &str = "HYPERLIQUID_TESTNET_PRIVATE_KEY";

// Precision constants
pub const HYPERLIQUID_DEFAULT_PRICE_PRECISION: u8 = 6;
pub const HYPERLIQUID_DEFAULT_SIZE_PRECISION: u8 = 8;

// WebSocket constants
pub const HYPERLIQUID_HEARTBEAT_INTERVAL_SECS: u64 = 30;
pub const HYPERLIQUID_RECONNECT_DELAY_SECS: u64 = 5;
