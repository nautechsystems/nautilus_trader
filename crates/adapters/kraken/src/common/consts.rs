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

//! Core constants shared across the Kraken adapter components.

use std::{num::NonZeroU32, sync::LazyLock};

use nautilus_model::identifiers::{ClientId, Venue};
use nautilus_network::ratelimiter::quota::Quota;
use ustr::Ustr;

/// Venue identifier string.
pub const KRAKEN: &str = "KRAKEN";

/// Static venue instance.
pub static KRAKEN_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(KRAKEN)));

/// Static client ID instance.
pub static KRAKEN_CLIENT_ID: LazyLock<ClientId> =
    LazyLock::new(|| ClientId::new(Ustr::from(KRAKEN)));

// API Partner integration identifier
pub const NAUTILUS_KRAKEN_BROKER_ID: &str = "AA98 N84G GOPN GL6Y";

// WebSocket-specific constants
pub const KRAKEN_PONG: &str = "pong";
pub const KRAKEN_WS_TOPIC_DELIMITER: char = '.';

// Spot API URLs (v2)
pub const KRAKEN_SPOT_HTTP_URL: &str = "https://api.kraken.com";
pub const KRAKEN_SPOT_WS_PUBLIC_URL: &str = "wss://ws.kraken.com/v2";
pub const KRAKEN_SPOT_WS_PRIVATE_URL: &str = "wss://ws-auth.kraken.com/v2";
pub const KRAKEN_SPOT_WS_L3_URL: &str = "wss://ws-l3.kraken.com/v2";

// Futures API URLs
pub const KRAKEN_FUTURES_HTTP_URL: &str = "https://futures.kraken.com";
pub const KRAKEN_FUTURES_WS_URL: &str = "wss://futures.kraken.com/ws/v1";

// Demo URLs
pub const KRAKEN_FUTURES_DEMO_HTTP_URL: &str = "https://demo-futures.kraken.com";
pub const KRAKEN_FUTURES_DEMO_WS_URL: &str = "wss://demo-futures.kraken.com/ws/v1";

// Spot order flags (oflags parameter values)
pub const KRAKEN_OFLAG_POST_ONLY: &str = "post";
pub const KRAKEN_OFLAG_QUOTE_QUANTITY: &str = "viqc";

/// Kraken Futures WebSocket request rate limit: 100 requests per 1 second.
///
/// Set to 90/sec (10% margin below the documented 100/sec hard cap).
///
/// <https://docs.kraken.com/api/docs/guides/futures-rate-limits/#websocket-limits>
pub static KRAKEN_FUTURES_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(90).expect("non-zero")).expect("valid constant")
});

/// Kraken Spot WebSocket request rate limit (conservative).
///
/// The Spot WS message rate limit is dynamic and varies depending on system load.
/// No fixed number is documented — the server returns `{"Error": "Exceeded msg rate"}`
/// when exceeded. This conservative quota should avoid hitting the limit under normal use.
///
/// <https://docs.kraken.com/api/docs/guides/spot-ratelimits>
pub static KRAKEN_SPOT_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(20).expect("non-zero"))
        .expect("valid constant")
        .allow_burst(NonZeroU32::new(10).expect("non-zero"))
});

/// Kraken Spot WebSocket order rate limit (conservative).
///
/// Shares the same dynamic connection-level message budget as subscriptions.
/// The matching engine enforces additional per-pair rate limits with decay
/// (thresholds: 60 Starter / 125 Intermediate / 180 Pro).
///
/// <https://docs.kraken.com/api/docs/guides/spot-ratelimits>
pub static KRAKEN_SPOT_WS_ORDER_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(10).expect("non-zero"))
        .expect("valid constant")
        .allow_burst(NonZeroU32::new(10).expect("non-zero"))
});

/// Pre-interned rate limit key for WebSocket subscription operations.
pub static KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from("subscription")]);

/// Pre-interned rate limit key for WebSocket order operations.
pub static KRAKEN_RATE_LIMIT_KEY_ORDER: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from("order")]);

// Post-only rejection reason strings
pub const KRAKEN_FUTURES_POST_ONLY_REJECT: &str = "post_order_failed_because_it_would_filled";
pub const KRAKEN_SPOT_POST_ONLY_REJECT: &str = "Post only order";
pub const KRAKEN_SPOT_POST_ONLY_ERROR: &str = "EOrder:Post only order";
