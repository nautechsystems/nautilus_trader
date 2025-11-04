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

//! Asterdex constants and configuration values.

/// Asterdex venue identifier
pub const ASTERDEX: &str = "ASTERDEX";

/// Base HTTP URL for Asterdex API
pub const ASTERDEX_HTTP_BASE_URL: &str = "https://fapi.asterdex.com";

/// Base WebSocket URL for Asterdex
pub const ASTERDEX_WS_BASE_URL: &str = "wss://fstream.asterdex.com/ws";

/// Default recv_window for authenticated requests (milliseconds)
pub const ASTERDEX_DEFAULT_RECV_WINDOW: u64 = 5000;

/// Maximum recv_window value (milliseconds)
pub const ASTERDEX_MAX_RECV_WINDOW: u64 = 60000;

/// Default price precision
pub const ASTERDEX_DEFAULT_PRICE_PRECISION: u8 = 8;

/// Default size precision
pub const ASTERDEX_DEFAULT_SIZE_PRECISION: u8 = 8;

/// WebSocket ping interval (seconds) - Asterdex uses 3-minute heartbeat
pub const ASTERDEX_WS_PING_INTERVAL_SECS: u64 = 60;

/// WebSocket connection timeout (seconds)
pub const ASTERDEX_WS_TIMEOUT_SECS: u64 = 30;
