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

//! Unified error handling for the AX Exchange adapter.
//!
//! This module provides a comprehensive error taxonomy that distinguishes between
//! retryable, non-retryable, and fatal errors, with proper context preservation
//! for debugging and operational monitoring.

use std::time::Duration;

use nautilus_network::http::HttpClientError;
use thiserror::Error;

/// The main error type for all AX Exchange adapter operations.
#[derive(Debug, Error)]
pub enum AxError {
    /// Errors that should be retried with backoff.
    #[error("Retryable error: {source}")]
    Retryable {
        #[source]
        source: AxRetryableError,
        /// Suggested retry after duration, if provided by the server.
        retry_after: Option<Duration>,
    },

    /// Errors that should not be retried.
    #[error("Non-retryable error: {source}")]
    NonRetryable {
        #[source]
        source: AxNonRetryableError,
    },

    /// Fatal errors that require intervention.
    #[error("Fatal error: {source}")]
    Fatal {
        #[source]
        source: AxFatalError,
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
pub enum AxRetryableError {
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
    ServerError { status: u16 },

    /// Network timeout.
    #[error("Request timed out after {duration:?}")]
    Timeout { duration: Duration },

    /// Connection failure.
    #[error("Connection failed: {reason}")]
    ConnectionFailed { reason: String },
}

/// Errors that should not be retried automatically.
#[derive(Debug, Error)]
pub enum AxNonRetryableError {
    /// Bad request (HTTP 400).
    #[error("Bad request: {message}")]
    BadRequest { message: String },

    /// Unauthorized (HTTP 401).
    #[error("Authentication failed: {message}")]
    Unauthorized { message: String },

    /// Forbidden (HTTP 403).
    #[error("Access forbidden: {message}")]
    Forbidden { message: String },

    /// Not found (HTTP 404).
    #[error("Resource not found: {message}")]
    NotFound { message: String },

    /// Invalid API response.
    #[error("Invalid API response: {message}")]
    InvalidResponse { message: String },

    /// Invalid parameters.
    #[error("Invalid parameters: {message}")]
    InvalidParameters { message: String },

    /// Order rejected.
    #[error("Order rejected: {message}")]
    OrderRejected { message: String },

    /// Insufficient funds.
    #[error("Insufficient funds: {message}")]
    InsufficientFunds { message: String },
}

/// Fatal errors that require manual intervention.
#[derive(Debug, Error)]
pub enum AxFatalError {
    /// API credentials are invalid or missing.
    #[error("Invalid or missing API credentials")]
    InvalidCredentials,

    /// Account suspended or restricted.
    #[error("Account suspended: {reason}")]
    AccountSuspended { reason: String },

    /// System misconfiguration.
    #[error("Configuration error: {message}")]
    SystemMisconfiguration { message: String },

    /// Unrecoverable parsing error.
    #[error("Unrecoverable parsing error: {message}")]
    ParseError { message: String },
}

impl AxError {
    /// Returns `true` if this error can be retried.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    /// Returns `true` if this error is fatal.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Fatal { .. })
    }

    /// Creates a JSON parsing error with optional raw data.
    #[must_use]
    pub fn json_parse(message: impl Into<String>, raw: Option<String>) -> Self {
        Self::Json {
            message: message.into(),
            raw,
        }
    }

    /// Creates a WebSocket error.
    #[must_use]
    pub fn websocket(message: impl Into<String>) -> Self {
        Self::WebSocket(message.into())
    }

    /// Creates a configuration error.
    #[must_use]
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }
}
