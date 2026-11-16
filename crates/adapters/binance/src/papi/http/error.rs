// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Binance Portfolio Margin (PAPI) HTTP error types.

use std::{fmt::Display, time::Duration};

use nautilus_network::http::error::HttpClientError;

use crate::common::error::{
    BINANCE_IP_BAN_STATUS, BINANCE_RATE_LIMIT_BLOCKED_MESSAGE, is_retryable_http_status,
    is_retryable_venue_code,
};

/// Binance Portfolio Margin HTTP client error type.
#[derive(Debug)]
pub enum BinancePapiHttpError {
    /// Missing or incomplete API credentials.
    MissingCredentials,
    /// The configured credentials use a signature scheme PAPI does not document.
    UnsupportedSignatureScheme(String),
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
    /// JSON parsing or serialization error.
    JsonError(String),
    /// Request validation error.
    ValidationError(String),
    /// Network or connection error.
    NetworkError(String),
    /// Request timed out; the outcome of a mutating request remains unknown until reconciled.
    Timeout(String),
    /// Request was canceled; the outcome of a mutating request remains unknown until reconciled.
    Canceled(String),
    /// The request is not sent because a cooldown or local quota blocks admission.
    RateLimitBlocked,
    /// The retry elapsed budget was exhausted.
    RetryBudgetExceeded(String),
    /// Unexpected HTTP status code.
    UnexpectedStatus {
        /// HTTP status code.
        status: u16,
        /// Response body.
        body: String,
        /// Venue-advertised minimum retry delay from the `Retry-After` header.
        retry_after: Option<Duration>,
    },
}

impl BinancePapiHttpError {
    /// Returns `true` if the error is transient and a read request can be retried.
    ///
    /// A retryable error on a mutating request still leaves an unknown outcome: PAPI writes are
    /// never blind-resubmitted locally, and the ambiguity is resolved by reconciliation.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::BinanceError {
                status: BINANCE_IP_BAN_STATUS,
                retry_after: None,
                ..
            }
            | Self::UnexpectedStatus {
                status: BINANCE_IP_BAN_STATUS,
                retry_after: None,
                ..
            } => false,
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

impl Display for BinancePapiHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials => f.write_str("Missing API credentials"),
            Self::UnsupportedSignatureScheme(scheme) => {
                write!(f, "Unsupported PAPI signature scheme: {scheme}")
            }
            Self::BinanceError {
                code,
                message,
                status,
                ..
            } => {
                write!(f, "Binance error {code} (HTTP {status}): {message}")
            }
            Self::JsonError(msg) => write!(f, "JSON error: {msg}"),
            Self::ValidationError(msg) => write!(f, "Validation error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::Canceled(msg) => write!(f, "Canceled: {msg}"),
            Self::RateLimitBlocked => f.write_str(BINANCE_RATE_LIMIT_BLOCKED_MESSAGE),
            Self::RetryBudgetExceeded(msg) => write!(f, "Retry budget exceeded: {msg}"),
            Self::UnexpectedStatus { status, body, .. } => {
                write!(f, "Unexpected status {status}: {body}")
            }
        }
    }
}

impl std::error::Error for BinancePapiHttpError {}

impl From<serde_json::Error> for BinancePapiHttpError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}

impl From<anyhow::Error> for BinancePapiHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<HttpClientError> for BinancePapiHttpError {
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

/// Result type for Binance Portfolio Margin HTTP operations.
pub type BinancePapiHttpResult<T> = Result<T, BinancePapiHttpError>;
