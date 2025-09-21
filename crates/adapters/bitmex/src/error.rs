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

//! Unified error handling for the BitMEX adapter.
//!
//! This module provides a comprehensive error taxonomy that distinguishes between
//! retryable, non-retryable, and fatal errors, with proper context preservation
//! for debugging and operational monitoring.

use std::time::Duration;

use nautilus_network::http::HttpClientError;
use reqwest::StatusCode;
use thiserror::Error;
use tokio_tungstenite::tungstenite;

/// The main error type for all BitMEX adapter operations.
#[derive(Debug, Error)]
pub enum BitmexError {
    /// Errors that should be retried with backoff.
    #[error("Retryable error: {source}")]
    Retryable {
        #[source]
        source: BitmexRetryableError,
        /// Suggested retry after duration, if provided by the server.
        retry_after: Option<Duration>,
    },

    /// Errors that should not be retried.
    #[error("Non-retryable error: {source}")]
    NonRetryable {
        #[source]
        source: BitmexNonRetryableError,
    },

    /// Fatal errors that require intervention.
    #[error("Fatal error: {source}")]
    Fatal {
        #[source]
        source: BitmexFatalError,
    },

    /// Network transport errors.
    #[error("Network error: {0}")]
    Network(#[from] HttpClientError),

    /// WebSocket specific errors.
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),

    /// JSON serialization/deserialization errors.
    #[error("JSON error: {message}")]
    Json {
        message: String,
        /// The raw JSON that failed to parse, if available.
        raw: Option<String>,
    },

    /// Configuration errors.
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Errors that should be retried with appropriate backoff.
#[derive(Debug, Error)]
pub enum BitmexRetryableError {
    /// Rate limit exceeded (HTTP 429).
    #[error("Rate limit exceeded (remaining: {remaining:?}, reset: {reset_at:?})")]
    RateLimit {
        remaining: Option<u32>,
        reset_at: Option<Duration>,
    },

    /// Service unavailable (HTTP 503).
    #[error("Service temporarily unavailable")]
    ServiceUnavailable,

    /// Gateway timeout (HTTP 504).
    #[error("Gateway timeout")]
    GatewayTimeout,

    /// Server error (HTTP 5xx).
    #[error("Server error (status: {status})")]
    ServerError { status: StatusCode },

    /// Network timeout.
    #[error("Request timed out after {duration:?}")]
    Timeout { duration: Duration },

    /// Temporary network issue.
    #[error("Temporary network error: {message}")]
    TemporaryNetwork { message: String },

    /// WebSocket connection lost.
    #[error("WebSocket connection lost")]
    ConnectionLost,

    /// Order book resync required.
    #[error("Order book resync required for {symbol}")]
    OrderBookResync { symbol: String },
}

/// Errors that should not be retried.
#[derive(Debug, Error)]
pub enum BitmexNonRetryableError {
    /// Bad request (HTTP 400).
    #[error("Bad request: {message}")]
    BadRequest { message: String },

    /// Not found (HTTP 404).
    #[error("Resource not found: {resource}")]
    NotFound { resource: String },

    /// Method not allowed (HTTP 405).
    #[error("Method not allowed: {method}")]
    MethodNotAllowed { method: String },

    /// Validation error.
    #[error("Validation error: {field}: {message}")]
    Validation { field: String, message: String },

    /// Invalid order parameters.
    #[error("Invalid order: {message}")]
    InvalidOrder { message: String },

    /// Insufficient balance.
    #[error("Insufficient balance: {available} < {required}")]
    InsufficientBalance { available: String, required: String },

    /// Symbol not found or invalid.
    #[error("Invalid symbol: {symbol}")]
    InvalidSymbol { symbol: String },

    /// Invalid API request format.
    #[error("Invalid request format: {message}")]
    InvalidRequest { message: String },

    /// Missing required parameter.
    #[error("Missing required parameter: {param}")]
    MissingParameter { param: String },

    /// Order not found.
    #[error("Order not found: {order_id}")]
    OrderNotFound { order_id: String },

    /// Position not found.
    #[error("Position not found: {symbol}")]
    PositionNotFound { symbol: String },
}

/// Fatal errors that require manual intervention.
#[derive(Debug, Error)]
pub enum BitmexFatalError {
    /// Authentication failed (HTTP 401).
    #[error("Authentication failed: {message}")]
    AuthenticationFailed { message: String },

    /// Forbidden (HTTP 403).
    #[error("Forbidden: {message}")]
    Forbidden { message: String },

    /// Account suspended.
    #[error("Account suspended: {reason}")]
    AccountSuspended { reason: String },

    /// Invalid API credentials.
    #[error("Invalid API credentials")]
    InvalidCredentials,

    /// API version no longer supported.
    #[error("API version no longer supported")]
    ApiVersionDeprecated,

    /// Critical invariant violation.
    #[error("Critical invariant violation: {invariant}")]
    InvariantViolation { invariant: String },
}

impl BitmexError {
    /// Creates a new rate limit error from HTTP headers.
    ///
    /// # Parameters
    ///
    /// - `remaining`: X-RateLimit-Remaining header value
    /// - `reset`: X-RateLimit-Reset header value (UNIX timestamp in seconds)
    /// - `retry_after`: Retry-After header value (seconds to wait)
    pub fn from_rate_limit_headers(
        remaining: Option<&str>,
        reset: Option<&str>,
        retry_after: Option<&str>,
    ) -> Self {
        let remaining = remaining.and_then(|s| s.parse().ok());

        // X-RateLimit-Reset is a UNIX timestamp, compute duration from now
        let reset_at = reset.and_then(|s| {
            s.parse::<u64>().ok().and_then(|timestamp| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_secs();
                if timestamp > now {
                    Some(Duration::from_secs(timestamp - now))
                } else {
                    Some(Duration::from_secs(0))
                }
            })
        });

        // Prefer explicit Retry-After header if present
        let retry_duration = retry_after
            .and_then(|s| s.parse::<u64>().ok().map(Duration::from_secs))
            .or(reset_at);

        Self::Retryable {
            source: BitmexRetryableError::RateLimit {
                remaining,
                reset_at,
            },
            retry_after: retry_duration,
        }
    }

    /// Creates an error from an HTTP status code and optional message.
    pub fn from_http_status(status: StatusCode, message: Option<String>) -> Self {
        match status {
            StatusCode::BAD_REQUEST => Self::NonRetryable {
                source: BitmexNonRetryableError::BadRequest {
                    message: message.unwrap_or_else(|| "Bad request".to_string()),
                },
            },
            StatusCode::UNAUTHORIZED => Self::Fatal {
                source: BitmexFatalError::AuthenticationFailed {
                    message: message.unwrap_or_else(|| "Unauthorized".to_string()),
                },
            },
            StatusCode::FORBIDDEN => Self::Fatal {
                source: BitmexFatalError::Forbidden {
                    message: message.unwrap_or_else(|| "Forbidden".to_string()),
                },
            },
            StatusCode::NOT_FOUND => Self::NonRetryable {
                source: BitmexNonRetryableError::NotFound {
                    resource: message.unwrap_or_else(|| "Resource".to_string()),
                },
            },
            StatusCode::METHOD_NOT_ALLOWED => Self::NonRetryable {
                source: BitmexNonRetryableError::MethodNotAllowed {
                    method: message.unwrap_or_else(|| "Method".to_string()),
                },
            },
            StatusCode::TOO_MANY_REQUESTS => Self::from_rate_limit_headers(None, None, None),
            StatusCode::SERVICE_UNAVAILABLE => Self::Retryable {
                source: BitmexRetryableError::ServiceUnavailable,
                retry_after: None,
            },
            StatusCode::GATEWAY_TIMEOUT => Self::Retryable {
                source: BitmexRetryableError::GatewayTimeout,
                retry_after: None,
            },
            s if s.is_server_error() => Self::Retryable {
                source: BitmexRetryableError::ServerError { status },
                retry_after: None,
            },
            _ => Self::NonRetryable {
                source: BitmexNonRetryableError::InvalidRequest {
                    message: format!("Unexpected status: {status}"),
                },
            },
        }
    }

    /// Checks if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    /// Checks if this error is fatal.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Fatal { .. })
    }

    /// Gets the suggested retry duration if available.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Retryable { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

// Re-export for backward compatibility during migration
pub use crate::http::error::BitmexBuildError;

impl From<serde_json::Error> for BitmexError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json {
            message: error.to_string(),
            raw: None,
        }
    }
}

impl From<BitmexBuildError> for BitmexError {
    fn from(error: BitmexBuildError) -> Self {
        Self::NonRetryable {
            source: BitmexNonRetryableError::Validation {
                field: "parameters".to_string(),
                message: error.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_error_classification() {
        let err = BitmexError::from_http_status(StatusCode::TOO_MANY_REQUESTS, None);
        assert!(err.is_retryable());
        assert!(!err.is_fatal());

        let err = BitmexError::from_http_status(StatusCode::UNAUTHORIZED, None);
        assert!(!err.is_retryable());
        assert!(err.is_fatal());

        let err = BitmexError::from_http_status(StatusCode::BAD_REQUEST, None);
        assert!(!err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_rate_limit_parsing() {
        // Use a timestamp far in the future to ensure retry_after is computed
        let future_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 60;
        let err = BitmexError::from_rate_limit_headers(
            Some("10"),
            Some(&future_timestamp.to_string()),
            None,
        );
        match err {
            BitmexError::Retryable {
                source: BitmexRetryableError::RateLimit { remaining, .. },
                retry_after,
                ..
            } => {
                assert_eq!(remaining, Some(10));
                assert!(retry_after.is_some());
                let duration = retry_after.unwrap();
                assert!(duration.as_secs() >= 59 && duration.as_secs() <= 61);
            }
            _ => panic!("Expected rate limit error"),
        }
    }

    #[rstest]
    fn test_rate_limit_with_retry_after() {
        let err = BitmexError::from_rate_limit_headers(Some("0"), None, Some("30"));
        match err {
            BitmexError::Retryable {
                source: BitmexRetryableError::RateLimit { remaining, .. },
                retry_after,
                ..
            } => {
                assert_eq!(remaining, Some(0));
                assert_eq!(retry_after, Some(Duration::from_secs(30)));
            }
            _ => panic!("Expected rate limit error"),
        }
    }

    #[rstest]
    fn test_retry_after() {
        let err = BitmexError::Retryable {
            source: BitmexRetryableError::RateLimit {
                remaining: Some(0),
                reset_at: Some(Duration::from_secs(60)),
            },
            retry_after: Some(Duration::from_secs(60)),
        };
        assert_eq!(err.retry_after(), Some(Duration::from_secs(60)));
    }
}
