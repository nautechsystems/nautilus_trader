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

//! Unified error handling for the Bybit adapter.
//!
//! This module provides a comprehensive error taxonomy that distinguishes between
//! retryable, non-retryable, and fatal errors, with proper context preservation
//! for debugging and operational monitoring.

use std::time::Duration;

use nautilus_network::http::{HttpClientError, HttpResponse};
use thiserror::Error;
use tokio_tungstenite::tungstenite;

/// The main error type for all Bybit adapter operations.
#[derive(Debug, Error)]
pub enum BybitError {
    /// Errors that should be retried with backoff.
    #[error("Retryable error: {source}")]
    Retryable {
        #[source]
        source: BybitRetryableError,
        /// Suggested retry after duration, if provided by the server.
        retry_after: Option<Duration>,
    },

    /// Errors that should not be retried.
    #[error("Non-retryable error: {source}")]
    NonRetryable {
        #[source]
        source: BybitNonRetryableError,
    },

    /// Fatal errors that require intervention.
    #[error("Fatal error: {source}")]
    Fatal {
        #[source]
        source: BybitFatalError,
    },

    /// Network transport errors.
    #[error("Network error: {0}")]
    Network(#[from] HttpClientError),

    /// WebSocket specific errors.
    #[error("WebSocket error: {0}")]
    WebSocket(String),

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
pub enum BybitRetryableError {
    /// Rate limit exceeded (HTTP 429).
    ///
    /// Bybit uses X-Bapi-Limit headers for rate limiting information.
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
    ServerError { status: u16 },

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
pub enum BybitNonRetryableError {
    /// Bad request (HTTP 400).
    ///
    /// Bybit returns retCode/retMsg in the response body for errors.
    #[error("Bad request: {message} (retCode: {ret_code:?})")]
    BadRequest {
        message: String,
        ret_code: Option<i32>,
    },

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
    #[error("Invalid order: {message} (retCode: {ret_code:?})")]
    InvalidOrder {
        message: String,
        ret_code: Option<i32>,
    },

    /// Insufficient balance.
    #[error("Insufficient balance: {message}")]
    InsufficientBalance { message: String },

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

    /// Bybit specific error codes.
    ///
    /// See <https://bybit-exchange.github.io/docs/v5/error> for error codes.
    #[error("Bybit error (retCode: {ret_code}): {message}")]
    BybitApiError { ret_code: i32, message: String },
}

/// Fatal errors that require manual intervention.
#[derive(Debug, Error)]
pub enum BybitFatalError {
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

    /// Permission denied for endpoint.
    #[error("Permission denied: {endpoint}")]
    PermissionDenied { endpoint: String },
}

impl BybitError {
    /// Creates a new rate limit error from HTTP headers.
    ///
    /// Bybit uses the following headers:
    /// - X-Bapi-Limit: Rate limit window
    /// - X-Bapi-Limit-Status: Current usage
    /// - X-Bapi-Limit-Reset-Timestamp: Reset time (Unix timestamp in ms)
    ///
    /// # Parameters
    ///
    /// - `limit_status`: X-Bapi-Limit-Status header value (e.g., "148")
    /// - `limit`: X-Bapi-Limit header value (e.g., "150")
    /// - `reset_timestamp`: X-Bapi-Limit-Reset-Timestamp header value (Unix ms)
    pub fn from_rate_limit_headers(
        limit_status: Option<&str>,
        limit: Option<&str>,
        reset_timestamp: Option<&str>,
    ) -> Self {
        let current = limit_status.and_then(|s| s.parse::<u32>().ok());
        let max_limit = limit.and_then(|s| s.parse::<u32>().ok());

        let remaining = if let (Some(current), Some(max)) = (current, max_limit) {
            Some(max.saturating_sub(current))
        } else {
            None
        };

        // X-Bapi-Limit-Reset-Timestamp is in milliseconds
        let reset_at = reset_timestamp.and_then(|s| {
            s.parse::<u64>().ok().and_then(|timestamp_ms| {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_millis() as u64;
                if timestamp_ms > now_ms {
                    Some(Duration::from_millis(timestamp_ms - now_ms))
                } else {
                    Some(Duration::from_secs(0))
                }
            })
        });

        Self::Retryable {
            source: BybitRetryableError::RateLimit {
                remaining,
                reset_at,
            },
            retry_after: reset_at,
        }
    }

    /// Creates an error from an HTTP status code and optional message.
    pub fn from_http_status(status: u16, message: Option<String>) -> Self {
        match status {
            400 => Self::NonRetryable {
                source: BybitNonRetryableError::BadRequest {
                    message: message.unwrap_or_else(|| "Bad request".to_string()),
                    ret_code: None,
                },
            },
            401 => Self::Fatal {
                source: BybitFatalError::AuthenticationFailed {
                    message: message.unwrap_or_else(|| "Unauthorized".to_string()),
                },
            },
            403 => Self::Fatal {
                source: BybitFatalError::Forbidden {
                    message: message.unwrap_or_else(|| "Forbidden".to_string()),
                },
            },
            404 => Self::NonRetryable {
                source: BybitNonRetryableError::NotFound {
                    resource: message.unwrap_or_else(|| "Resource".to_string()),
                },
            },
            405 => Self::NonRetryable {
                source: BybitNonRetryableError::MethodNotAllowed {
                    method: message.unwrap_or_else(|| "Method".to_string()),
                },
            },
            429 => Self::from_rate_limit_headers(None, None, None),
            503 => Self::Retryable {
                source: BybitRetryableError::ServiceUnavailable,
                retry_after: None,
            },
            504 => Self::Retryable {
                source: BybitRetryableError::GatewayTimeout,
                retry_after: None,
            },
            s if (500..600).contains(&s) => Self::Retryable {
                source: BybitRetryableError::ServerError { status: s },
                retry_after: None,
            },
            _ => Self::NonRetryable {
                source: BybitNonRetryableError::InvalidRequest {
                    message: format!("Unexpected status: {status}"),
                },
            },
        }
    }

    /// Creates an error from an HTTP response.
    pub fn from_http_response(response: &HttpResponse) -> Self {
        let status = response.status.as_u16();
        let message = String::from_utf8_lossy(&response.body).to_string();
        Self::from_http_status(status, Some(message))
    }

    /// Creates an error from a Bybit API response with retCode.
    ///
    /// Bybit returns errors with retCode and retMsg fields.
    /// See <https://bybit-exchange.github.io/docs/v5/error> for error codes.
    pub fn from_bybit_ret_code(ret_code: i32, message: String) -> Self {
        match ret_code {
            0 => Self::Config("Success code received as error".to_string()),
            10001 => Self::NonRetryable {
                source: BybitNonRetryableError::BadRequest {
                    message,
                    ret_code: Some(ret_code),
                },
            },
            10002 | 10003 | 10004 | 33004 => Self::Fatal {
                source: BybitFatalError::AuthenticationFailed { message },
            },
            10005 => Self::Fatal {
                source: BybitFatalError::PermissionDenied { endpoint: message },
            },
            10006 => Self::from_rate_limit_headers(None, None, None),
            110001 | 110003 | 110004 => Self::NonRetryable {
                source: BybitNonRetryableError::InvalidOrder {
                    message,
                    ret_code: Some(ret_code),
                },
            },
            110007 => Self::NonRetryable {
                source: BybitNonRetryableError::InsufficientBalance { message },
            },
            110025 | 110026 => Self::NonRetryable {
                source: BybitNonRetryableError::OrderNotFound { order_id: message },
            },
            110043 => Self::NonRetryable {
                source: BybitNonRetryableError::InvalidSymbol { symbol: message },
            },
            _ if (10000..20000).contains(&ret_code) => Self::NonRetryable {
                source: BybitNonRetryableError::BybitApiError { ret_code, message },
            },
            // Specific retryable errors (system busy/frequency protection)
            10429 | 131230 | 148019 => Self::Retryable {
                source: BybitRetryableError::TemporaryNetwork { message },
                retry_after: None,
            },
            // 30000-39999: Institutional Lending, trading restrictions (non-retryable)
            _ if (30000..40000).contains(&ret_code) => Self::NonRetryable {
                source: BybitNonRetryableError::BybitApiError { ret_code, message },
            },
            // 130000-139999: Withdrawal, KYC, account restrictions (non-retryable)
            _ if (130000..140000).contains(&ret_code) => Self::NonRetryable {
                source: BybitNonRetryableError::BybitApiError { ret_code, message },
            },
            // 170000-179999: Spot trading business errors (non-retryable)
            _ if (170000..180000).contains(&ret_code) => Self::NonRetryable {
                source: BybitNonRetryableError::BybitApiError { ret_code, message },
            },
            // 180000-189999: Earn product errors (non-retryable)
            _ if (180000..190000).contains(&ret_code) => Self::NonRetryable {
                source: BybitNonRetryableError::BybitApiError { ret_code, message },
            },
            // Default: treat unknown errors as non-retryable (fail-safe)
            _ => Self::NonRetryable {
                source: BybitNonRetryableError::BybitApiError { ret_code, message },
            },
        }
    }

    /// Checks if this error is retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    /// Checks if this error is fatal.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Fatal { .. })
    }

    /// Gets the suggested retry duration if available.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Retryable { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

// Re-export existing error types for backward compatibility
pub use crate::{
    http::error::BybitHttpError,
    websocket::error::{BybitWsError, BybitWsResult},
};

impl From<serde_json::Error> for BybitError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json {
            message: error.to_string(),
            raw: None,
        }
    }
}

impl From<tungstenite::Error> for BybitError {
    fn from(error: tungstenite::Error) -> Self {
        Self::WebSocket(error.to_string())
    }
}

impl From<BybitHttpError> for BybitError {
    fn from(error: BybitHttpError) -> Self {
        match error {
            BybitHttpError::MissingCredentials => {
                Self::Config("API credentials not configured".to_string())
            }
            BybitHttpError::BybitError {
                error_code,
                message,
            } => Self::Config(format!("Bybit error {error_code}: {message}")),
            BybitHttpError::JsonError(msg) => Self::Json {
                message: msg,
                raw: None,
            },
            BybitHttpError::ValidationError(msg) => {
                Self::Config(format!("Validation error: {msg}"))
            }
            BybitHttpError::BuildError(e) => Self::Config(format!("Build error: {e}")),
            BybitHttpError::Canceled(msg) => Self::Config(format!("Request canceled: {msg}")),
            BybitHttpError::NetworkError(msg) => Self::Config(format!("Network error: {msg}")),
            BybitHttpError::UnexpectedStatus { status, body } => Self::Json {
                message: format!("HTTP {status}: {body}"),
                raw: Some(body),
            },
        }
    }
}

impl From<BybitWsError> for BybitError {
    fn from(error: BybitWsError) -> Self {
        Self::WebSocket(error.to_string())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_error_classification() {
        let err = BybitError::from_http_status(429, None);
        assert!(err.is_retryable());
        assert!(!err.is_fatal());

        let err = BybitError::from_http_status(401, None);
        assert!(!err.is_retryable());
        assert!(err.is_fatal());

        let err = BybitError::from_http_status(400, None);
        assert!(!err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_rate_limit_parsing() {
        // Simulate future timestamp
        let future_timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 60_000; // 60 seconds in future

        let err = BybitError::from_rate_limit_headers(
            Some("148"),
            Some("150"),
            Some(&future_timestamp_ms.to_string()),
        );

        match err {
            BybitError::Retryable {
                source: BybitRetryableError::RateLimit { remaining, .. },
                retry_after,
                ..
            } => {
                assert_eq!(remaining, Some(2)); // 150 - 148 = 2
                assert!(retry_after.is_some());
                let duration = retry_after.unwrap();
                assert!(duration.as_secs() >= 59 && duration.as_secs() <= 61);
            }
            _ => panic!("Expected rate limit error"),
        }
    }

    #[rstest]
    fn test_bybit_ret_codes() {
        // Authentication error
        let err = BybitError::from_bybit_ret_code(10003, "Invalid API key".to_string());
        assert!(err.is_fatal());

        // Bad request
        let err = BybitError::from_bybit_ret_code(10001, "Invalid parameter".to_string());
        assert!(!err.is_retryable());
        assert!(!err.is_fatal());

        // Rate limit
        let err = BybitError::from_bybit_ret_code(10006, "Rate limit exceeded".to_string());
        assert!(err.is_retryable());

        // Insufficient balance
        let err = BybitError::from_bybit_ret_code(110007, "Not enough balance".to_string());
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_retry_after() {
        let err = BybitError::Retryable {
            source: BybitRetryableError::RateLimit {
                remaining: Some(0),
                reset_at: Some(Duration::from_secs(60)),
            },
            retry_after: Some(Duration::from_secs(60)),
        };
        assert_eq!(err.retry_after(), Some(Duration::from_secs(60)));
    }

    #[rstest]
    fn test_invalid_order_errors() {
        let err = BybitError::from_bybit_ret_code(110001, "Invalid order".to_string());
        match err {
            BybitError::NonRetryable {
                source: BybitNonRetryableError::InvalidOrder { ret_code, .. },
            } => {
                assert_eq!(ret_code, Some(110001));
            }
            _ => panic!("Expected invalid order error"),
        }
    }
}
