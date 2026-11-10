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

//! Venue identifiers and protocol constants for the Kalshi adapter.

use std::{sync::LazyLock, time::Duration};

use nautilus_model::identifiers::{ClientId, Venue};
use ustr::Ustr;

/// Venue name string for Kalshi.
pub const KALSHI: &str = "KALSHI";

pub(crate) const PRICE_PRECISION: u8 = 6;
pub(crate) const SIZE_PRECISION: u8 = 2;

/// Kalshi client identifier.
pub static KALSHI_CLIENT_ID: LazyLock<ClientId> =
    LazyLock::new(|| ClientId::new(Ustr::from(KALSHI)));

/// Kalshi venue identifier.
pub static KALSHI_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(KALSHI)));

/// Path prefix for the REST trading API.
pub const KALSHI_API_PATH_PREFIX: &str = "/trade-api/v2";

/// Path signed for WebSocket authentication.
pub const KALSHI_WS_SIGN_PATH: &str = "/trade-api/ws/v2";

/// API key ID header name.
pub const KALSHI_ACCESS_KEY_HEADER: &str = "KALSHI-ACCESS-KEY";

/// Request signature header name.
pub const KALSHI_ACCESS_SIGNATURE_HEADER: &str = "KALSHI-ACCESS-SIGNATURE";

/// Request timestamp header name.
pub const KALSHI_ACCESS_TIMESTAMP_HEADER: &str = "KALSHI-ACCESS-TIMESTAMP";

/// Default HTTP request timeout.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Kalshi sends WebSocket Ping frames every 10 seconds.
pub const WS_PING_INTERVAL: Duration = Duration::from_secs(10);

/// Base reconnect backoff for the WebSocket client.
pub const RECONNECT_BASE_BACKOFF: Duration = Duration::from_millis(250);

/// Maximum reconnect backoff for the WebSocket client.
pub const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Basic-tier read token refill per second.
pub const BASIC_READ_TOKENS_PER_SEC: u32 = 200;

/// Basic-tier write token refill per second.
pub const BASIC_WRITE_TOKENS_PER_SEC: u32 = 100;

/// Maximum page size accepted by public trades pagination.
pub const MAX_TRADES_PAGE_SIZE: u32 = 1000;
