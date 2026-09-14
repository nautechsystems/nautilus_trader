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

//! Binance Spot HTTP error types.

use std::{fmt::Display, time::Duration};

use nautilus_network::http::error::HttpClientError;

use crate::common::error::{is_retryable_http_status, is_retryable_venue_code};
// Re-export unified SBE decode error
pub use crate::spot::sbe::SbeDecodeError;

/// Binance Spot HTTP client error type.
#[derive(Debug)]
pub enum BinanceSpotHttpError {
    /// Missing API credentials for authenticated request.
    MissingCredentials,
    /// Binance API returned an error response.
    BinanceError {
        /// Binance error code.
        code: i64,
        /// Error message from Binance.
        message: String,
        /// HTTP status of the error response.
        status: u16,
        /// Venue-advertised minimum retry delay from the `Retry-After` header.
        retry_after: Option<Duration>,
    },
    /// SBE decode error.
    SbeDecodeError(SbeDecodeError),
    /// JSON decode error.
    JsonError(String),
    /// Response parse error after a venue response was received.
    ResponseParseError(String),
    /// Request validation error.
    ValidationError(String),
    /// Network or connection error.
    NetworkError(String),
    /// Request timed out.
    Timeout(String),
    /// Request was canceled.
    Canceled(String),
    /// The retry elapsed budget was exhausted.
    RetryBudgetExceeded(String),
    /// Unexpected HTTP status code.
    UnexpectedStatus {
        /// HTTP status code.
        status: u16,
        /// Response body (hex encoded for SBE).
        body: String,
        /// Venue-advertised minimum retry delay from the `Retry-After` header.
        retry_after: Option<Duration>,
    },
}

impl BinanceSpotHttpError {
    /// Returns `true` if the error is transient and the operation can be retried.
    ///
    /// Retryability is independent of command-outcome classification: a retryable error on a
    /// state-changing command still leaves an unknown outcome at the execution boundary.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::NetworkError(_) | Self::Timeout(_) => true,
            Self::BinanceError { code, status, .. } => {
                is_retryable_venue_code(*code) || is_retryable_http_status(*status)
            }
            Self::UnexpectedStatus { status, .. } => is_retryable_http_status(*status),
            _ => false,
        }
    }

    /// Returns the venue-advertised minimum retry delay, when present.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::BinanceError { retry_after, .. } | Self::UnexpectedStatus { retry_after, .. } => {
                *retry_after
            }
            _ => None,
        }
    }
}

impl Display for BinanceSpotHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "Missing API credentials"),
            Self::BinanceError {
                code,
                message,
                status,
                ..
            } => {
                write!(f, "Binance error {code} (HTTP {status}): {message}")
            }
            Self::SbeDecodeError(err) => write!(f, "SBE decode error: {err}"),
            Self::JsonError(msg) => write!(f, "JSON decode error: {msg}"),
            Self::ResponseParseError(msg) => write!(f, "Response parse error: {msg}"),
            Self::ValidationError(msg) => write!(f, "Validation error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::Canceled(msg) => write!(f, "Canceled: {msg}"),
            Self::RetryBudgetExceeded(msg) => write!(f, "Retry budget exceeded: {msg}"),
            Self::UnexpectedStatus { status, body, .. } => {
                write!(f, "Unexpected status {status}: {body}")
            }
        }
    }
}

impl std::error::Error for BinanceSpotHttpError {}

impl From<SbeDecodeError> for BinanceSpotHttpError {
    fn from(err: SbeDecodeError) -> Self {
        Self::SbeDecodeError(err)
    }
}

impl From<anyhow::Error> for BinanceSpotHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<HttpClientError> for BinanceSpotHttpError {
    fn from(err: HttpClientError) -> Self {
        match err {
            HttpClientError::TimeoutError(msg) => Self::Timeout(msg),
            HttpClientError::InvalidProxy(msg) | HttpClientError::ClientBuildError(msg) => {
                Self::NetworkError(msg)
            }
            HttpClientError::Error(msg) | HttpClientError::TransportError(msg) => {
                Self::NetworkError(msg)
            }
        }
    }
}

/// Result type for Binance Spot HTTP operations.
pub type BinanceSpotHttpResult<T> = Result<T, BinanceSpotHttpError>;

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn venue_error(code: i64, status: u16) -> BinanceSpotHttpError {
        BinanceSpotHttpError::BinanceError {
            code,
            message: "venue message".to_string(),
            status,
            retry_after: None,
        }
    }

    #[rstest]
    #[case::network(BinanceSpotHttpError::NetworkError("connection reset".to_string()), true)]
    #[case::timeout(BinanceSpotHttpError::Timeout("timed out".to_string()), true)]
    #[case::missing_credentials(BinanceSpotHttpError::MissingCredentials, false)]
    #[case::validation(BinanceSpotHttpError::ValidationError("bad param".to_string()), false)]
    #[case::canceled(BinanceSpotHttpError::Canceled("shutdown".to_string()), false)]
    #[case::budget(
        BinanceSpotHttpError::RetryBudgetExceeded("exceeded".to_string()),
        false
    )]
    #[case::parse(BinanceSpotHttpError::ResponseParseError("bad body".to_string()), false)]
    fn test_is_retryable_transport_and_local(
        #[case] error: BinanceSpotHttpError,
        #[case] expected: bool,
    ) {
        assert_eq!(error.is_retryable(), expected);
    }

    #[rstest]
    #[case::rate_limit_code(venue_error(-1003, 429), true)]
    #[case::order_rate_limit_code(venue_error(-1015, 400), true)]
    #[case::status_500(venue_error(-1000, 500), true)]
    #[case::status_418(venue_error(-1003, 418), true)]
    #[case::permanent_venue_code(venue_error(-1100, 400), false)]
    #[case::auth_code(venue_error(-2015, 401), false)]
    #[case::timestamp_drift(venue_error(-1021, 400), false)]
    fn test_is_retryable_venue_errors(#[case] error: BinanceSpotHttpError, #[case] expected: bool) {
        assert_eq!(error.is_retryable(), expected);
    }

    #[rstest]
    #[case::status_429(BinanceSpotHttpError::UnexpectedStatus {
        status: 429,
        body: "rate limited".to_string(),
        retry_after: None,
    }, true)]
    #[case::status_500(BinanceSpotHttpError::UnexpectedStatus {
        status: 500,
        body: "internal server error".to_string(),
        retry_after: None,
    }, true)]
    #[case::status_401(BinanceSpotHttpError::UnexpectedStatus {
        status: 401,
        body: "unauthorized".to_string(),
        retry_after: None,
    }, false)]
    #[case::status_400(BinanceSpotHttpError::UnexpectedStatus {
        status: 400,
        body: "bad request".to_string(),
        retry_after: None,
    }, false)]
    fn test_is_retryable_unexpected_status(
        #[case] error: BinanceSpotHttpError,
        #[case] expected: bool,
    ) {
        assert_eq!(error.is_retryable(), expected);
    }

    #[rstest]
    fn test_retry_after_accessor() {
        let delay = Duration::from_secs(2);
        let error = BinanceSpotHttpError::BinanceError {
            code: -1003,
            message: "Too many requests".to_string(),
            status: 429,
            retry_after: Some(delay),
        };
        assert_eq!(error.retry_after(), Some(delay));
        assert_eq!(
            BinanceSpotHttpError::NetworkError("x".to_string()).retry_after(),
            None
        );
    }

    #[rstest]
    fn test_display_includes_status() {
        let err = venue_error(-1100, 400);
        let msg = err.to_string();
        assert!(msg.contains("-1100"));
        assert!(msg.contains("400"));
    }
}
