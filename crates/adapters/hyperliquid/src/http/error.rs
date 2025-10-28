// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use thiserror::Error;

/// Comprehensive error type for Hyperliquid operations
#[derive(Debug, Error)]
pub enum Error {
    /// Transport layer errors (network, connection issues)
    #[error("transport error: {0}")]
    Transport(String),

    /// JSON serialization/deserialization errors
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    /// Authentication errors (signature mismatch, wrong wallet)
    #[error("auth error: {0}")]
    Auth(String),

    /// Rate limiting errors with optional retry information
    #[error("Rate limited on {scope} (weight={weight}) retry_after_ms={retry_after_ms:?}")]
    RateLimit {
        scope: &'static str,
        weight: u32,
        retry_after_ms: Option<u64>,
    },

    /// Nonce window violations (nonces must be within time window and unique)
    #[error("nonce window error: {0}")]
    NonceWindow(String),

    /// Bad request errors (client-side invalid payload)
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Exchange-specific errors from Hyperliquid server
    #[error("exchange error: {0}")]
    Exchange(String),

    /// Request timeout
    #[error("timeout")]
    Timeout,

    /// Message decoding/parsing errors
    #[error("decode error: {0}")]
    Decode(String),

    /// Invariant violation (impossible state)
    #[error("invariant violated: {0}")]
    Invariant(&'static str),

    /// HTTP errors with status code
    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String },

    /// URL parsing errors
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    /// Standard IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Create a transport error
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    /// Create an auth error
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    /// Create a rate limit error
    pub fn rate_limit(scope: &'static str, weight: u32, retry_after_ms: Option<u64>) -> Self {
        Self::RateLimit {
            scope,
            weight,
            retry_after_ms,
        }
    }

    /// Create a nonce window error
    pub fn nonce_window(msg: impl Into<String>) -> Self {
        Self::NonceWindow(msg.into())
    }

    /// Create a bad request error
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    /// Create an exchange error
    pub fn exchange(msg: impl Into<String>) -> Self {
        Self::Exchange(msg.into())
    }

    /// Create a decode error
    pub fn decode(msg: impl Into<String>) -> Self {
        Self::Decode(msg.into())
    }

    /// Create an HTTP error
    pub fn http(status: u16, message: impl Into<String>) -> Self {
        Self::Http {
            status,
            message: message.into(),
        }
    }

    /// Create an error from HTTP status code and body
    pub fn from_http_status(status: reqwest::StatusCode, body: &[u8]) -> Self {
        let message = String::from_utf8_lossy(body).to_string();
        match status.as_u16() {
            401 | 403 => Self::auth(format!("HTTP {}: {}", status.as_u16(), message)),
            400 => Self::bad_request(format!("HTTP {}: {}", status.as_u16(), message)),
            429 => Self::rate_limit("unknown", 0, None),
            500..=599 => Self::exchange(format!("HTTP {}: {}", status.as_u16(), message)),
            _ => Self::http(status.as_u16(), message),
        }
    }

    /// Map reqwest errors to appropriate error types
    pub fn from_reqwest(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if let Some(status) = error.status() {
            let status_code = status.as_u16();
            match status_code {
                401 | 403 => Self::auth(format!("HTTP {}: authentication failed", status_code)),
                400 => Self::bad_request(format!("HTTP {}: bad request", status_code)),
                429 => Self::rate_limit("unknown", 0, None),
                500..=599 => Self::exchange(format!("HTTP {}: server error", status_code)),
                _ => Self::http(status_code, format!("HTTP error: {}", error)),
            }
        } else if error.is_connect() || error.is_request() {
            Self::transport(format!("Request error: {}", error))
        } else {
            Self::transport(format!("Unknown reqwest error: {}", error))
        }
    }

    /// Map HTTP client errors to appropriate error types
    pub fn from_http_client(error: nautilus_network::http::HttpClientError) -> Self {
        Self::transport(format!("HTTP client error: {}", error))
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Timeout | Self::RateLimit { .. } => true,
            Self::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }

    /// Check if error is due to rate limiting
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    /// Check if error is due to authentication issues
    pub fn is_auth_error(&self) -> bool {
        matches!(self, Self::Auth(_))
    }
}

/// Result type alias for Hyperliquid operations
pub type Result<T> = std::result::Result<T, Error>;

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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

        let auth_err = Error::auth("Invalid signature");
        assert!(auth_err.is_auth_error());

        let rate_limit_err = Error::rate_limit("test", 30, Some(30000));
        assert!(rate_limit_err.is_rate_limited());
        assert!(rate_limit_err.is_retryable());

        let http_err = Error::http(500, "Internal server error");
        assert!(http_err.is_retryable());
    }

    #[rstest]
    fn test_error_display() {
        let err = Error::RateLimit {
            scope: "info",
            weight: 20,
            retry_after_ms: Some(60000),
        };
        assert_eq!(
            err.to_string(),
            "Rate limited on info (weight=20) retry_after_ms=Some(60000)"
        );

        let err = Error::NonceWindow("Nonce too old".to_string());
        assert_eq!(err.to_string(), "nonce window error: Nonce too old");
    }

    #[rstest]
    fn test_retryable_errors() {
        assert!(Error::transport("test").is_retryable());
        assert!(Error::Timeout.is_retryable());
        assert!(Error::rate_limit("test", 10, None).is_retryable());
        assert!(Error::http(500, "server error").is_retryable());

        assert!(!Error::auth("test").is_retryable());
        assert!(!Error::bad_request("test").is_retryable());
        assert!(!Error::decode("test").is_retryable());
    }
}
