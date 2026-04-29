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

//! HTTP error types for the Polymarket adapter.

use nautilus_network::http::{HttpClientError, ReqwestError, StatusCode};
use thiserror::Error;

/// Error type for Polymarket HTTP operations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("transport error: {0}")]
    Transport(String),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("Rate limited on {scope} (weight={weight}) retry_after_ms={retry_after_ms:?}")]
    RateLimit {
        scope: &'static str,
        weight: u32,
        retry_after_ms: Option<u64>,
    },

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("exchange error: {0}")]
    Exchange(String),

    #[error("timeout")]
    Timeout,

    #[error("decode error: {0}")]
    Decode(String),

    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String },

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    pub fn rate_limit(scope: &'static str, weight: u32, retry_after_ms: Option<u64>) -> Self {
        Self::RateLimit {
            scope,
            weight,
            retry_after_ms,
        }
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn exchange(msg: impl Into<String>) -> Self {
        Self::Exchange(msg.into())
    }

    pub fn decode(msg: impl Into<String>) -> Self {
        Self::Decode(msg.into())
    }

    pub fn http(status: u16, message: impl Into<String>) -> Self {
        Self::Http {
            status,
            message: message.into(),
        }
    }

    /// Classifies an HTTP status code and body into the appropriate error variant.
    pub fn from_http_status(status: StatusCode, body: &[u8]) -> Self {
        let message = String::from_utf8_lossy(body).to_string();
        match status.as_u16() {
            401 | 403 => Self::auth(format!("HTTP {}: {message}", status.as_u16())),
            400 => Self::bad_request(format!("HTTP {}: {message}", status.as_u16())),
            429 => Self::rate_limit("unknown", 0, None),
            _ => Self::http(status.as_u16(), message),
        }
    }

    /// Classifies a raw status code (as `u16`) and body into the appropriate error variant.
    pub fn from_status_code(status: u16, body: &[u8]) -> Self {
        let message = String::from_utf8_lossy(body).to_string();
        match status {
            401 | 403 => Self::auth(format!("HTTP {status}: {message}")),
            400 => Self::bad_request(format!("HTTP {status}: {message}")),
            429 => Self::rate_limit("unknown", 0, None),
            _ => Self::http(status, message),
        }
    }

    /// Classifies a reqwest error into the appropriate error variant.
    #[expect(clippy::needless_pass_by_value)]
    pub fn from_reqwest(error: ReqwestError) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if let Some(status) = error.status() {
            let status_code = status.as_u16();
            match status_code {
                401 | 403 => Self::auth(format!("HTTP {status_code}: authentication failed")),
                400 => Self::bad_request(format!("HTTP {status_code}: bad request")),
                429 => Self::rate_limit("unknown", 0, None),
                _ => Self::http(status_code, format!("HTTP error: {error}")),
            }
        } else if error.is_connect() || error.is_request() {
            Self::transport(format!("Request error: {error}"))
        } else {
            Self::transport(format!("Unknown reqwest error: {error}"))
        }
    }

    #[expect(clippy::needless_pass_by_value)]
    pub fn from_http_client(error: HttpClientError) -> Self {
        Self::transport(format!("HTTP client error: {error}"))
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Timeout | Self::RateLimit { .. } => true,
            Self::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    pub fn is_auth_error(&self) -> bool {
        matches!(self, Self::Auth(_))
    }

    /// Returns `true` if this error originated from an HTTP status code response
    /// (as opposed to transport, timeout, or local errors).
    pub fn is_http_status_error(&self) -> bool {
        matches!(
            self,
            Self::Auth(_) | Self::BadRequest(_) | Self::RateLimit { .. } | Self::Http { .. }
        )
    }
}

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
            scope: "order",
            weight: 10,
            retry_after_ms: Some(60000),
        };
        assert_eq!(
            err.to_string(),
            "Rate limited on order (weight=10) retry_after_ms=Some(60000)"
        );
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
