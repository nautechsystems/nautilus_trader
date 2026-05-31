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

//! HTTP error taxonomy for Lighter REST responses.

use nautilus_network::http::HttpClientError;
use thiserror::Error;

/// Result alias for Lighter HTTP operations.
pub type LighterHttpResult<T> = Result<T, LighterHttpError>;

/// Errors emitted by the Lighter HTTP client.
#[derive(Debug, Clone, Error)]
pub enum LighterHttpError {
    /// Network-level failure (transport, DNS, TLS).
    #[error("network error: {0}")]
    Network(String),
    /// HTTP-level failure with status code and body.
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    /// Rate limit exceeded.
    #[error("rate limit exceeded: {0}")]
    RateLimit(String),
    /// Venue returned a structured error code.
    #[error("venue error {code}: {message}")]
    Venue { code: i64, message: String },
    /// Failed to parse a venue response.
    #[error("parse error: {0}")]
    Parse(String),
}

impl From<HttpClientError> for LighterHttpError {
    fn from(error: HttpClientError) -> Self {
        Self::Network(error.to_string())
    }
}

impl From<serde_json::Error> for LighterHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::Parse(error.to_string())
    }
}

impl From<anyhow::Error> for LighterHttpError {
    fn from(error: anyhow::Error) -> Self {
        Self::Parse(error.to_string())
    }
}

/// Returns `true` if a request producing this error should be retried.
///
/// Retryable shapes are transport-layer failures, server-side 5xx, and rate limits.
/// Venue-semantic errors (4xx other than 429, `Venue`, `Parse`) are surfaced unchanged.
#[must_use]
pub fn should_retry_lighter_http_error(error: &LighterHttpError) -> bool {
    match error {
        LighterHttpError::Network(_) | LighterHttpError::RateLimit(_) => true,
        LighterHttpError::Http { status, .. } => *status >= 500,
        LighterHttpError::Venue { .. } | LighterHttpError::Parse(_) => false,
    }
}

/// Constructs a transport-shaped error for retry-manager timeout / cancellation paths.
#[must_use]
pub fn create_lighter_http_timeout_error(msg: String) -> LighterHttpError {
    LighterHttpError::Network(msg)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::network_retries(LighterHttpError::Network("dns failure".into()), true)]
    #[case::rate_limit_retries(LighterHttpError::RateLimit("429".into()), true)]
    #[case::server_5xx_retries(LighterHttpError::Http { status: 503, body: "busy".into() }, true)]
    #[case::server_500_retries(LighterHttpError::Http { status: 500, body: "boom".into() }, true)]
    #[case::client_400_does_not_retry(LighterHttpError::Http { status: 400, body: "bad".into() }, false)]
    #[case::client_404_does_not_retry(LighterHttpError::Http { status: 404, body: "missing".into() }, false)]
    #[case::venue_does_not_retry(LighterHttpError::Venue { code: 20001, message: "invalid".into() }, false)]
    #[case::parse_does_not_retry(LighterHttpError::Parse("bad json".into()), false)]
    fn test_should_retry_lighter_http_error(
        #[case] error: LighterHttpError,
        #[case] expected: bool,
    ) {
        assert_eq!(should_retry_lighter_http_error(&error), expected);
    }
}
