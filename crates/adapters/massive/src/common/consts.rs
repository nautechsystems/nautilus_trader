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

use std::{sync::LazyLock, time::Duration};

use nautilus_model::identifiers::{ClientId, Venue};
use ustr::Ustr;

/// Venue identifier string.
pub const MASSIVE: &str = "MASSIVE";

/// Static venue instance.
pub static MASSIVE_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(MASSIVE)));

/// Static client ID instance.
pub static MASSIVE_CLIENT_ID: LazyLock<ClientId> =
    LazyLock::new(|| ClientId::new(Ustr::from(MASSIVE)));

/// Environment variable holding the Massive API key.
pub const MASSIVE_API_KEY_ENV: &str = "MASSIVE_API_KEY";

pub const REST_URL: &str = "https://api.massive.com";
/// Real-time US stocks WebSocket cluster.
pub const WS_URL_REALTIME: &str = "wss://socket.massive.com/stocks";
/// 15-minute delayed US stocks WebSocket cluster.
pub const WS_URL_DELAYED: &str = "wss://delayed.massive.com/stocks";

/// Maximum page size accepted by the `/v3/reference/tickers` endpoint.
pub const TICKERS_PAGE_LIMIT: u32 = 1000;

/// Maximum page size accepted by the `/v2/aggs` endpoints.
pub const AGGS_PAGE_LIMIT: u32 = 50_000;

/// Maximum page size accepted by the `/v3/trades` and `/v3/quotes` endpoints.
pub const TICKS_PAGE_LIMIT: u32 = 50_000;

/// Maximum tickers per `ticker.any_of` reference query, bounded by URL length.
pub const TICKERS_ANY_OF_CHUNK: usize = 100;

pub const HTTP_TIMEOUT: Duration = Duration::from_secs(60);

/// WebSocket control-frame ping interval, in seconds.
pub const WS_HEARTBEAT_SECS: u64 = 30;

pub const RECONNECT_BASE_BACKOFF: Duration = Duration::from_millis(250);
pub const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);
pub const RECONNECT_JITTER_MS: u64 = 200;
pub const RECONNECT_BACKOFF_FACTOR: f64 = 2.0;
pub const RECONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum time the client waits for the feed handler task to drain on
/// disconnect before forcibly aborting.
pub const WS_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_venue_constant() {
        assert_eq!(MASSIVE_VENUE.as_str(), MASSIVE);
        assert_eq!(MASSIVE_CLIENT_ID.as_str(), MASSIVE);
    }

    #[rstest]
    fn test_url_constants() {
        assert!(REST_URL.starts_with("https://"));
        assert!(WS_URL_REALTIME.starts_with("wss://"));
        assert!(WS_URL_DELAYED.starts_with("wss://"));
    }

    #[rstest]
    fn test_page_limits() {
        assert_eq!(TICKERS_PAGE_LIMIT, 1000);
        assert_eq!(AGGS_PAGE_LIMIT, 50_000);
        assert_eq!(TICKS_PAGE_LIMIT, 50_000);
    }
}
