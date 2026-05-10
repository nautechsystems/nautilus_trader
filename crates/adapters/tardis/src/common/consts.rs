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

//! Tardis adapter constants.

use std::{num::NonZeroU32, sync::LazyLock};

use nautilus_network::ratelimiter::quota::Quota;

/// The Tardis adapter identifier string.
pub const TARDIS: &str = "TARDIS";

/// Environment variable name for the Tardis API key.
pub const TARDIS_API_KEY: &str = "TARDIS_API_KEY";

/// Environment variable name for the Tardis Machine WebSocket URL.
pub const TARDIS_MACHINE_WS_URL: &str = "TARDIS_MACHINE_WS_URL";

/// Rate limit key for Tardis REST API requests.
pub const TARDIS_REST_RATE_KEY: &str = "tardis_rest";

/// Default rate limit for Tardis REST API (10 requests per second).
pub static TARDIS_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(10).expect("non-zero")).expect("valid quota")
});

/// Maximum reconnection delay for the Tardis Machine WebSocket in seconds.
pub const WS_MAX_RECONNECT_DELAY_SECS: u64 = 30;

/// Initial reconnection delay for the Tardis Machine WebSocket in seconds.
pub const WS_INITIAL_RECONNECT_DELAY_SECS: u64 = 1;

/// Heartbeat (ping) interval for the Tardis Machine WebSocket in seconds.
pub const WS_HEARTBEAT_INTERVAL_SECS: u64 = 10;
