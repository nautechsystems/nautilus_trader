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

//! Adapter-level error types aggregating HTTP and WebSocket errors.

use std::fmt::Display;

use crate::common::consts::{BINANCE_STATUS_UNKNOWN_CODE, BINANCE_UNEXPECTED_RESPONSE_CODE};

/// Binance WebSocket streams error type shared by spot and futures clients.
#[derive(Debug)]
pub enum BinanceWsError {
    /// General client error.
    ClientError(String),
    /// Authentication failed.
    AuthenticationError(String),
    /// Message parsing error.
    ParseError(String),
    /// Network or connection error.
    NetworkError(String),
    /// Operation timed out.
    Timeout(String),
}

impl Display for BinanceWsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientError(msg) => write!(f, "Client error: {msg}"),
            Self::AuthenticationError(msg) => write!(f, "Authentication error: {msg}"),
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
        }
    }
}

impl std::error::Error for BinanceWsError {}

/// Result type for Binance WebSocket stream operations.
pub type BinanceWsResult<T> = Result<T, BinanceWsError>;

/// Adapter-level error aggregating HTTP, WebSocket, and SBE errors.
#[derive(Debug, thiserror::Error)]
pub enum BinanceError {
    /// A Spot HTTP API error.
    #[error("Spot HTTP error: {0}")]
    SpotHttp(#[from] crate::spot::http::error::BinanceSpotHttpError),

    /// A Futures HTTP API error.
    #[error("Futures HTTP error: {0}")]
    FuturesHttp(#[from] crate::futures::http::error::BinanceFuturesHttpError),

    /// A WebSocket streams error (spot or futures).
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] BinanceWsError),

    /// A Spot WebSocket Trading API error.
    #[error("Spot WS API error: {0}")]
    SpotWsApi(#[from] crate::spot::websocket::trading::error::BinanceWsApiError),

    /// A Futures WebSocket Trading API error.
    #[error("Futures WS API error: {0}")]
    FuturesWsApi(#[from] crate::futures::websocket::trading::error::BinanceFuturesWsApiError),

    /// A configuration or build error.
    #[error("Config error: {0}")]
    Config(String),
}

/// Binance error codes indicating rate limiting or throttling.
const BINANCE_RATE_LIMIT_ERROR_CODES: [i64; 2] = [
    -1003, // Too many requests; WAF limit violated
    -1015, // Too many new orders; rate limit violated
];

/// Returns `true` when the venue error code marks a transient rate-limit failure.
pub(crate) fn is_retryable_venue_code(code: i64) -> bool {
    BINANCE_RATE_LIMIT_ERROR_CODES.contains(&code)
}

/// Returns `true` when the HTTP status marks a transient failure (rate limited,
/// auto-banned, or a server error).
pub(crate) fn is_retryable_http_status(status: u16) -> bool {
    status == 429 || status == 418 || status >= 500
}

/// Returns `true` when the venue error code means execution status is unknown.
///
/// Binance documents -1006 (unexpected matching-engine response) and -1007 (backend
/// timeout) as "send status unknown; execution status unknown" for any request.
pub(crate) fn is_ambiguous_venue_code(code: i64) -> bool {
    code == BINANCE_UNEXPECTED_RESPONSE_CODE || code == BINANCE_STATUS_UNKNOWN_CODE
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::too_many_requests(-1003, true)]
    #[case::too_many_new_orders(-1015, true)]
    #[case::illegal_characters(-1100, false)]
    #[case::invalid_api_key(-2015, false)]
    #[case::invalid_signature(-1022, false)]
    #[case::unexpected_response(-1006, false)]
    fn test_is_retryable_venue_code(#[case] code: i64, #[case] expected: bool) {
        assert_eq!(is_retryable_venue_code(code), expected);
    }

    #[rstest]
    #[case::rate_limited(429, true)]
    #[case::banned(418, true)]
    #[case::server_error(500, true)]
    #[case::bad_gateway(502, true)]
    #[case::bad_request(400, false)]
    #[case::unauthorized(401, false)]
    #[case::forbidden(403, false)]
    #[case::success(200, false)]
    fn test_is_retryable_http_status(#[case] status: u16, #[case] expected: bool) {
        assert_eq!(is_retryable_http_status(status), expected);
    }

    #[rstest]
    #[case::unexpected_response(-1006, true)]
    #[case::status_unknown(-1007, true)]
    #[case::rate_limit(-1003, false)]
    #[case::no_such_order(-2013, false)]
    fn test_is_ambiguous_venue_code(#[case] code: i64, #[case] expected: bool) {
        assert_eq!(is_ambiguous_venue_code(code), expected);
    }
}
