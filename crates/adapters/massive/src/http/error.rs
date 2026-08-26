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

use nautilus_network::http::HttpClientError;
use thiserror::Error;

/// Error type for Massive operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Transport layer errors (network, connection issues).
    #[error("transport error: {0}")]
    Transport(String),

    /// JSON serialization/deserialization errors.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    /// Authentication errors (missing or invalid API key).
    #[error("auth error: {0}")]
    Auth(String),

    /// Rate limiting errors (HTTP 429).
    #[error("Rate limited (retry_after_ms={retry_after_ms:?})")]
    RateLimit { retry_after_ms: Option<u64> },

    /// Bad request errors (client-side invalid payload).
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Server-side errors from Massive.
    #[error("venue error: {0}")]
    Venue(String),

    /// Request timeout.
    #[error("timeout")]
    Timeout,

    /// Message decoding/parsing errors.
    #[error("decode error: {0}")]
    Decode(String),

    /// HTTP errors with status code.
    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String },

    /// URL parsing errors.
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),
}

impl Error {
    /// Creates a transport error.
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    /// Creates an auth error.
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    /// Creates a rate limit error.
    #[must_use]
    pub fn rate_limit(retry_after_ms: Option<u64>) -> Self {
        Self::RateLimit { retry_after_ms }
    }

    /// Creates a bad request error.
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    /// Creates a venue error.
    pub fn venue(msg: impl Into<String>) -> Self {
        Self::Venue(msg.into())
    }

    /// Creates a decode error.
    pub fn decode(msg: impl Into<String>) -> Self {
        Self::Decode(msg.into())
    }

    /// Creates an HTTP error.
    pub fn http(status: u16, message: impl Into<String>) -> Self {
        Self::Http {
            status,
            message: message.into(),
        }
    }

    /// Creates an error from HTTP status code and body.
    #[must_use]
    pub fn from_http_status(status: u16, body: &[u8]) -> Self {
        let message = String::from_utf8_lossy(body).to_string();
        match status {
            401 | 403 => Self::auth(format!("HTTP {status}: {message}")),
            400 => Self::bad_request(format!("HTTP {status}: {message}")),
            429 => Self::rate_limit(None),
            500..=599 => Self::venue(format!("HTTP {status}: {message}")),
            _ => Self::http(status, message),
        }
    }

    /// Maps HTTP client errors to appropriate error types.
    #[expect(clippy::needless_pass_by_value)]
    pub fn from_http_client(error: HttpClientError) -> Self {
        Self::transport(format!("HTTP client error: {error}"))
    }

    /// Returns true if the error is retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Timeout | Self::RateLimit { .. } | Self::Venue(_) => true,
            Self::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }

    /// Returns true if the error is due to rate limiting.
    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    /// Returns true if the error is due to authentication.
    #[must_use]
    pub fn is_auth_error(&self) -> bool {
        matches!(self, Self::Auth(_))
    }
}

/// Result type alias for Massive operations.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(401, true, false, false)]
    #[case(403, true, false, false)]
    #[case(400, false, false, false)]
    #[case(429, false, true, true)]
    #[case(500, false, false, true)]
    #[case(503, false, false, true)]
    #[case(404, false, false, false)]
    fn test_from_http_status_classification(
        #[case] status: u16,
        #[case] expect_auth: bool,
        #[case] expect_rate_limit: bool,
        #[case] expect_retryable: bool,
    ) {
        let err = Error::from_http_status(status, b"test body");
        assert_eq!(err.is_auth_error(), expect_auth, "is_auth for {status}");
        assert_eq!(
            err.is_rate_limited(),
            expect_rate_limit,
            "is_rate_limited for {status}"
        );
        assert_eq!(
            err.is_retryable(),
            expect_retryable,
            "is_retryable for {status}"
        );
    }

    #[rstest]
    fn test_retryable_errors() {
        assert!(Error::transport("test").is_retryable());
        assert!(Error::Timeout.is_retryable());
        assert!(Error::rate_limit(None).is_retryable());
        assert!(Error::venue("server error").is_retryable());
        assert!(!Error::auth("test").is_retryable());
        assert!(!Error::bad_request("test").is_retryable());
        assert!(!Error::decode("test").is_retryable());
    }
}
