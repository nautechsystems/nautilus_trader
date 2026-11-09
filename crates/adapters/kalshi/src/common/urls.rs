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

//! Base URL resolution for Kalshi REST and WebSocket endpoints.

use super::enums::KalshiEnvironment;

const KALSHI_PRODUCTION_HTTP_URL: &str = "https://external-api.kalshi.com/trade-api/v2";
const KALSHI_PRODUCTION_WS_URL: &str = "wss://external-api-ws.kalshi.com/trade-api/ws/v2";

const KALSHI_DEMO_HTTP_URL: &str = "https://external-api.demo.kalshi.co/trade-api/v2";
const KALSHI_DEMO_WS_URL: &str = "wss://external-api-ws.demo.kalshi.co/trade-api/ws/v2";

/// Returns the REST base URL for the given environment.
#[must_use]
pub const fn kalshi_http_base_url(environment: KalshiEnvironment) -> &'static str {
    match environment {
        KalshiEnvironment::Production => KALSHI_PRODUCTION_HTTP_URL,
        KalshiEnvironment::Demo => KALSHI_DEMO_HTTP_URL,
    }
}

/// Returns the WebSocket URL for the given environment.
#[must_use]
pub const fn kalshi_ws_url(environment: KalshiEnvironment) -> &'static str {
    match environment {
        KalshiEnvironment::Production => KALSHI_PRODUCTION_WS_URL,
        KalshiEnvironment::Demo => KALSHI_DEMO_WS_URL,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_http_base_url() {
        assert_eq!(
            kalshi_http_base_url(KalshiEnvironment::Production),
            KALSHI_PRODUCTION_HTTP_URL,
        );
        assert_eq!(
            kalshi_http_base_url(KalshiEnvironment::Demo),
            KALSHI_DEMO_HTTP_URL,
        );
    }

    #[rstest]
    fn test_ws_url() {
        assert_eq!(
            kalshi_ws_url(KalshiEnvironment::Production),
            KALSHI_PRODUCTION_WS_URL,
        );
        assert_eq!(kalshi_ws_url(KalshiEnvironment::Demo), KALSHI_DEMO_WS_URL,);
    }
}
