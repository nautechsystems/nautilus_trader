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

use nautilus_network::http::{HttpClientError, ReqwestError};
use thiserror::Error;

/// Error type for Coinbase operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Transport layer errors (network, connection issues).
    #[error("transport error: {0}")]
    Transport(String),

    /// JSON serialization/deserialization errors.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    /// Authentication errors (invalid key, expired JWT).
    #[error("auth error: {0}")]
    Auth(String),

    /// Rate limiting errors.
    #[error("Rate limited (retry_after_ms={retry_after_ms:?})")]
    RateLimit { retry_after_ms: Option<u64> },

    /// Bad request errors (client-side invalid payload).
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Exchange-specific errors from Coinbase server.
    #[error("exchange error: {0}")]
    Exchange(String),

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

    /// Standard IO errors.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
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
    pub fn rate_limit(retry_after_ms: Option<u64>) -> Self {
        Self::RateLimit { retry_after_ms }
    }

    /// Creates a bad request error.
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    /// Creates an exchange error.
    pub fn exchange(msg: impl Into<String>) -> Self {
        Self::Exchange(msg.into())
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
    pub fn from_http_status(status: u16, body: &[u8]) -> Self {
        let message = String::from_utf8_lossy(body).to_string();
        match status {
            401 | 403 => Self::auth(format!("HTTP {status}: {message}")),
            400 => Self::bad_request(format!("HTTP {status}: {message}")),
            429 => Self::rate_limit(None),
            500..=599 => Self::exchange(format!("HTTP {status}: {message}")),
            _ => Self::http(status, message),
        }
    }

    /// Maps reqwest errors to appropriate error types.
    #[expect(clippy::needless_pass_by_value)]
    pub fn from_reqwest(error: ReqwestError) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if let Some(status) = error.status() {
            let status_code = status.as_u16();
            match status_code {
                401 | 403 => Self::auth(format!("HTTP {status_code}: authentication failed")),
                400 => Self::bad_request(format!("HTTP {status_code}: bad request")),
                429 => Self::rate_limit(None),
                500..=599 => Self::exchange(format!("HTTP {status_code}: server error")),
                _ => Self::http(status_code, format!("HTTP error: {error}")),
            }
        } else if error.is_connect() || error.is_request() {
            Self::transport(format!("Request error: {error}"))
        } else {
            Self::transport(format!("Unknown reqwest error: {error}"))
        }
    }

    /// Maps HTTP client errors to appropriate error types.
    #[expect(clippy::needless_pass_by_value)]
    pub fn from_http_client(error: HttpClientError) -> Self {
        Self::transport(format!("HTTP client error: {error}"))
    }

    /// Returns true if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Timeout | Self::RateLimit { .. } | Self::Exchange(_) => true,
            Self::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }

    /// Returns true if the error is due to rate limiting.
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    /// Returns true if the error is due to authentication.
    pub fn is_auth_error(&self) -> bool {
        matches!(self, Self::Auth(_))
    }
}

/// Result type alias for Coinbase operations.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_error_constructors() {
        let transport_err = Error::transport("Connection failed");
        assert!(matches!(transport_err, Error::Transport(_)));
        assert_eq!(
            transport_err.to_string(),
            "transport error: Connection failed"
        );

        let auth_err = Error::auth("Invalid JWT");
        assert!(auth_err.is_auth_error());

        let rate_limit_err = Error::rate_limit(Some(30000));
        assert!(rate_limit_err.is_rate_limited());
        assert!(rate_limit_err.is_retryable());

        let http_err = Error::http(500, "Internal server error");
        assert!(http_err.is_retryable());
    }

    #[rstest]
    fn test_retryable_errors() {
        assert!(Error::transport("test").is_retryable());
        assert!(Error::Timeout.is_retryable());
        assert!(Error::rate_limit(None).is_retryable());
        assert!(Error::http(500, "server error").is_retryable());
        assert!(Error::exchange("server error").is_retryable());

        assert!(!Error::auth("test").is_retryable());
        assert!(!Error::bad_request("test").is_retryable());
        assert!(!Error::decode("test").is_retryable());
    }

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
    fn test_error_display() {
        let err = Error::RateLimit {
            retry_after_ms: Some(60000),
        };
        assert_eq!(err.to_string(), "Rate limited (retry_after_ms=Some(60000))");
    }
}
