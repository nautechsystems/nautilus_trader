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

//! Gate.io constants and configuration values.

/// Gate.io venue identifier
pub const GATEIO: &str = "GATEIO";

/// Base HTTP URL for Gate.io API v4 (mainnet)
pub const GATEIO_HTTP_BASE_URL: &str = "https://api.gateio.ws/api/v4";

/// Base WebSocket URL for Gate.io spot (mainnet)
pub const GATEIO_WS_SPOT_URL: &str = "wss://api.gateio.ws/ws/v4/";

/// Base WebSocket URL for Gate.io futures (mainnet)
pub const GATEIO_WS_FUTURES_URL: &str = "wss://fx-ws.gateio.ws/v4/ws/usdt";

/// Base WebSocket URL for Gate.io options (mainnet)
pub const GATEIO_WS_OPTIONS_URL: &str = "wss://op-ws.gateio.ws/v4/ws/btc";

/// Default price precision for Gate.io instruments
pub const GATEIO_DEFAULT_PRICE_PRECISION: u8 = 8;

/// Default size precision for Gate.io instruments
pub const GATEIO_DEFAULT_SIZE_PRECISION: u8 = 8;

/// Maximum number of subscriptions per WebSocket connection
pub const GATEIO_MAX_SUBSCRIPTIONS: usize = 100;

/// Rate limit: 200 requests per 10 seconds for most endpoints
pub const GATEIO_RATE_LIMIT_DEFAULT: usize = 200;
pub const GATEIO_RATE_WINDOW_SECS: u64 = 10;

/// Rate limit for spot orders: 10 per second
pub const GATEIO_RATE_LIMIT_SPOT_ORDERS: usize = 10;

/// Rate limit for futures orders: 100 per second
pub const GATEIO_RATE_LIMIT_FUTURES_ORDERS: usize = 100;

/// WebSocket ping interval in seconds
pub const GATEIO_WS_PING_INTERVAL_SECS: u64 = 20;

/// WebSocket connection timeout in seconds
pub const GATEIO_WS_TIMEOUT_SECS: u64 = 30;
