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

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

use super::enums::BulletEnvironment;

pub const BULLET: &str = "BULLET";
pub static BULLET_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(BULLET)));

// REST base URLs
pub const BULLET_HTTP_URL: &str = "https://tradingapi.bullet.xyz";
pub const BULLET_TESTNET_HTTP_URL: &str = "https://tradingapi.testnet.bullet.xyz";
pub const BULLET_STAGING_HTTP_URL: &str = "https://tradingapi.staging.bullet.xyz";

// WebSocket URLs
pub const BULLET_WS_URL: &str = "wss://tradingapi.bullet.xyz/ws";
pub const BULLET_TESTNET_WS_URL: &str = "wss://tradingapi.testnet.bullet.xyz/ws";
pub const BULLET_STAGING_WS_URL: &str = "wss://tradingapi.staging.bullet.xyz/ws";

/// Returns the HTTP base URL for the given environment.
pub fn http_url(environment: BulletEnvironment) -> &'static str {
    match environment {
        BulletEnvironment::Mainnet => BULLET_HTTP_URL,
        BulletEnvironment::Testnet => BULLET_TESTNET_HTTP_URL,
        BulletEnvironment::Staging => BULLET_STAGING_HTTP_URL,
    }
}

/// Returns the WebSocket URL for the given environment.
pub fn ws_url(environment: BulletEnvironment) -> &'static str {
    match environment {
        BulletEnvironment::Mainnet => BULLET_WS_URL,
        BulletEnvironment::Testnet => BULLET_TESTNET_WS_URL,
        BulletEnvironment::Staging => BULLET_STAGING_WS_URL,
    }
}

// Connection tuning
// Server closes idle connections after ~60s; ping every 30s
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const RECONNECT_BASE_BACKOFF: Duration = Duration::from_millis(250);
pub const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

// Default transaction gas limits (see Bullet docs/tx-fields.md)
pub const DEFAULT_MAX_FEE: u128 = 1u128 << 48;
pub const DEFAULT_PRIORITY_FEE_BIPS: u64 = 0;

// Error message substrings for detecting specific rejection reasons
pub const BULLET_POST_ONLY_WOULD_MATCH: &str = "post only would cross";

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_http_url() {
        assert_eq!(http_url(BulletEnvironment::Mainnet), BULLET_HTTP_URL);
        assert_eq!(http_url(BulletEnvironment::Testnet), BULLET_TESTNET_HTTP_URL);
        assert_eq!(http_url(BulletEnvironment::Staging), BULLET_STAGING_HTTP_URL);
    }

    #[rstest]
    fn test_ws_url() {
        assert_eq!(ws_url(BulletEnvironment::Mainnet), BULLET_WS_URL);
        assert_eq!(ws_url(BulletEnvironment::Testnet), BULLET_TESTNET_WS_URL);
        assert_eq!(ws_url(BulletEnvironment::Staging), BULLET_STAGING_WS_URL);
    }
}
