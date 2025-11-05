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

//! Hyperliquid constants and configuration values.

/// Hyperliquid venue identifier
pub const HYPERLIQUID: &str = "HYPERLIQUID";

/// Base HTTP URL for Hyperliquid API (mainnet)
pub const HYPERLIQUID_HTTP_BASE_URL: &str = "https://api.hyperliquid.xyz";

/// Base HTTP URL for Hyperliquid testnet
pub const HYPERLIQUID_HTTP_TESTNET_URL: &str = "https://api.hyperliquid-testnet.xyz";

/// Base WebSocket URL for Hyperliquid (mainnet)
pub const HYPERLIQUID_WS_BASE_URL: &str = "wss://api.hyperliquid.xyz/ws";

/// Base WebSocket URL for Hyperliquid testnet
pub const HYPERLIQUID_WS_TESTNET_URL: &str = "wss://api.hyperliquid-testnet.xyz/ws";

/// Info endpoint path
pub const HYPERLIQUID_INFO_PATH: &str = "/info";

/// Exchange endpoint path
pub const HYPERLIQUID_EXCHANGE_PATH: &str = "/exchange";

/// Default price precision
pub const HYPERLIQUID_DEFAULT_PRICE_PRECISION: u8 = 5;

/// Default size precision
pub const HYPERLIQUID_DEFAULT_SIZE_PRECISION: u8 = 6;

/// Maximum candles per request
pub const HYPERLIQUID_MAX_CANDLES: usize = 5000;

/// Maximum fills per request
pub const HYPERLIQUID_MAX_FILLS: usize = 2000;

/// Maximum orders per request
pub const HYPERLIQUID_MAX_ORDERS: usize = 2000;

/// WebSocket ping interval (seconds)
pub const HYPERLIQUID_WS_PING_INTERVAL_SECS: u64 = 30;

/// WebSocket connection timeout (seconds)
pub const HYPERLIQUID_WS_TIMEOUT_SECS: u64 = 30;
