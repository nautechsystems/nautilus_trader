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

//! Constants for the Lighter adapter.

use ustr::Ustr;

/// Lighter venue name.
pub const LIGHTER: &str = "LIGHTER";

/// Lighter mainnet HTTP API base URL.
pub const LIGHTER_MAINNET_HTTP_URL: &str = "https://mainnet.zklighter.elliot.ai";

/// Lighter testnet HTTP API base URL.
pub const LIGHTER_TESTNET_HTTP_URL: &str = "https://testnet.zklighter.elliot.ai";

/// Lighter mainnet WebSocket URL.
pub const LIGHTER_MAINNET_WS_URL: &str = "wss://mainnet.zklighter.elliot.ai/ws";

/// Lighter testnet WebSocket URL.
pub const LIGHTER_TESTNET_WS_URL: &str = "wss://testnet.zklighter.elliot.ai/ws";

/// Default price precision for Lighter instruments.
pub const LIGHTER_DEFAULT_PRICE_PRECISION: u8 = 8;

/// Default size precision for Lighter instruments.
pub const LIGHTER_DEFAULT_SIZE_PRECISION: u8 = 8;

/// WebSocket ping interval in seconds.
pub const WS_PING_INTERVAL_SECS: u64 = 20;

/// WebSocket reconnect delay in milliseconds.
pub const WS_RECONNECT_DELAY_MS: u64 = 5000;

/// Maximum WebSocket message size (10 MB).
pub const WS_MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// HTTP timeout in seconds.
pub const HTTP_TIMEOUT_SECS: u64 = 30;

/// Maximum retry attempts for HTTP requests.
pub const MAX_RETRIES: u32 = 3;

/// Initial retry delay in milliseconds.
pub const RETRY_DELAY_INITIAL_MS: u64 = 1000;

/// Maximum retry delay in milliseconds.
pub const RETRY_DELAY_MAX_MS: u64 = 10_000;

/// Lighter venue as Ustr.
pub fn lighter_venue() -> Ustr {
    Ustr::from(LIGHTER)
}
