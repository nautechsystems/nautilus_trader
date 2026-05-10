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

//! Adapter-level error types aggregating HTTP and WebSocket errors.

use std::fmt::Display;

/// Binance WebSocket streams error type shared by spot and futures clients.
#[derive(Debug)]
pub enum BinanceWsError {
    /// General client error.
    ClientError(String),
    /// Authentication failed.
    AuthenticationError(String),
    /// Message parsing error.
    ParseError(String),
    /// Network or connection error.
    NetworkError(String),
    /// Operation timed out.
    Timeout(String),
}

impl Display for BinanceWsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientError(msg) => write!(f, "Client error: {msg}"),
            Self::AuthenticationError(msg) => write!(f, "Authentication error: {msg}"),
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
        }
    }
}

impl std::error::Error for BinanceWsError {}

/// Result type for Binance WebSocket stream operations.
pub type BinanceWsResult<T> = Result<T, BinanceWsError>;

/// Adapter-level error aggregating HTTP, WebSocket, and SBE errors.
#[derive(Debug, thiserror::Error)]
pub enum BinanceError {
    /// A Spot HTTP API error.
    #[error("Spot HTTP error: {0}")]
    SpotHttp(#[from] crate::spot::http::error::BinanceSpotHttpError),

    /// A Futures HTTP API error.
    #[error("Futures HTTP error: {0}")]
    FuturesHttp(#[from] crate::futures::http::error::BinanceFuturesHttpError),

    /// A WebSocket streams error (spot or futures).
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] BinanceWsError),

    /// A Spot WebSocket Trading API error.
    #[error("Spot WS API error: {0}")]
    SpotWsApi(#[from] crate::spot::websocket::trading::error::BinanceWsApiError),

    /// A Futures WebSocket Trading API error.
    #[error("Futures WS API error: {0}")]
    FuturesWsApi(#[from] crate::futures::websocket::trading::error::BinanceFuturesWsApiError),

    /// A configuration or build error.
    #[error("Config error: {0}")]
    Config(String),
}

/// Binance error codes indicating authentication or permission failures.
const BINANCE_AUTH_ERROR_CODES: [i64; 3] = [
    -2015, // Invalid API-key, IP, or permissions for action
    -2014, // API-key format invalid
    -1022, // Signature for this request is not valid
];

/// Binance error codes indicating rate limiting or throttling.
const BINANCE_RATE_LIMIT_ERROR_CODES: [i64; 2] = [
    -1003, // Too many requests; WAF limit violated
    -1015, // Too many new orders; rate limit violated
];

impl BinanceError {
    /// Returns `true` if the error is likely transient and the operation can be retried.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::SpotHttp(e) => match e {
                crate::spot::http::error::BinanceSpotHttpError::NetworkError(_)
                | crate::spot::http::error::BinanceSpotHttpError::Timeout(_) => true,
                crate::spot::http::error::BinanceSpotHttpError::BinanceError { code, .. } => {
                    BINANCE_RATE_LIMIT_ERROR_CODES.contains(code)
                }
                crate::spot::http::error::BinanceSpotHttpError::UnexpectedStatus {
                    status, ..
                } => *status == 429 || *status >= 500,
                _ => false,
            },
            Self::FuturesHttp(e) => match e {
                crate::futures::http::error::BinanceFuturesHttpError::NetworkError(_)
                | crate::futures::http::error::BinanceFuturesHttpError::Timeout(_) => true,
                crate::futures::http::error::BinanceFuturesHttpError::BinanceError {
                    code, ..
                } => BINANCE_RATE_LIMIT_ERROR_CODES.contains(code),
                crate::futures::http::error::BinanceFuturesHttpError::UnexpectedStatus {
                    status,
                    ..
                } => *status == 429 || *status >= 500,
                _ => false,
            },
            Self::WebSocket(e) => matches!(
                e,
                BinanceWsError::NetworkError(_) | BinanceWsError::Timeout(_)
            ),
            Self::SpotWsApi(e) => matches!(
                e,
                crate::spot::websocket::trading::error::BinanceWsApiError::ConnectionError(_)
                    | crate::spot::websocket::trading::error::BinanceWsApiError::Timeout(_)
            ),
            Self::FuturesWsApi(e) => matches!(
                e,
                crate::futures::websocket::trading::error::BinanceFuturesWsApiError::ConnectionError(_)
            ),
            Self::Config(_) => false,
        }
    }

    /// Returns `true` if the error is fatal and requires intervention.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        match self {
            Self::SpotHttp(e) => match e {
                crate::spot::http::error::BinanceSpotHttpError::MissingCredentials => true,
                crate::spot::http::error::BinanceSpotHttpError::BinanceError { code, .. } => {
                    BINANCE_AUTH_ERROR_CODES.contains(code)
                }
                crate::spot::http::error::BinanceSpotHttpError::UnexpectedStatus {
                    status, ..
                } => *status == 401 || *status == 403,
                _ => false,
            },
            Self::FuturesHttp(e) => match e {
                crate::futures::http::error::BinanceFuturesHttpError::MissingCredentials => true,
                crate::futures::http::error::BinanceFuturesHttpError::BinanceError {
                    code, ..
                } => BINANCE_AUTH_ERROR_CODES.contains(code),
                crate::futures::http::error::BinanceFuturesHttpError::UnexpectedStatus {
                    status,
                    ..
                } => *status == 401 || *status == 403,
                _ => false,
            },
            Self::WebSocket(e) => {
                matches!(e, BinanceWsError::AuthenticationError(_))
            }
            Self::SpotWsApi(e) => matches!(
                e,
                crate::spot::websocket::trading::error::BinanceWsApiError::AuthenticationError(_)
            ),
            Self::FuturesWsApi(_) => false,
            Self::Config(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        futures::http::error::BinanceFuturesHttpError, spot::http::error::BinanceSpotHttpError,
    };

    #[rstest]
    fn test_spot_http_network_error_is_retryable() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::NetworkError(
            "connection reset".to_string(),
        ));
        assert!(err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_spot_http_timeout_is_retryable() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::Timeout("timed out".to_string()));
        assert!(err.is_retryable());
    }

    #[rstest]
    fn test_spot_http_missing_credentials_is_fatal() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::MissingCredentials);
        assert!(err.is_fatal());
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_spot_http_binance_error_is_not_retryable() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::BinanceError {
            code: -1021,
            message: "Timestamp for this request was 1000ms ahead".to_string(),
        });
        assert!(!err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_futures_http_network_error_is_retryable() {
        let err = BinanceError::FuturesHttp(BinanceFuturesHttpError::NetworkError(
            "connection refused".to_string(),
        ));
        assert!(err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_futures_http_missing_credentials_is_fatal() {
        let err = BinanceError::FuturesHttp(BinanceFuturesHttpError::MissingCredentials);
        assert!(err.is_fatal());
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_ws_auth_error_is_fatal() {
        let err = BinanceError::WebSocket(BinanceWsError::AuthenticationError(
            "invalid key".to_string(),
        ));
        assert!(err.is_fatal());
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_ws_network_error_is_retryable() {
        let err =
            BinanceError::WebSocket(BinanceWsError::NetworkError("connection lost".to_string()));
        assert!(err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_config_error_is_fatal() {
        let err = BinanceError::Config("invalid product type".to_string());
        assert!(err.is_fatal());
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_spot_http_auth_error_code_is_fatal() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::BinanceError {
            code: -2015,
            message: "Invalid API-key, IP, or permissions for action".to_string(),
        });
        assert!(err.is_fatal());
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_futures_http_auth_error_code_is_fatal() {
        let err = BinanceError::FuturesHttp(BinanceFuturesHttpError::BinanceError {
            code: -2015,
            message: "Invalid API-key".to_string(),
        });
        assert!(err.is_fatal());
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_spot_http_invalid_signature_is_fatal() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::BinanceError {
            code: -1022,
            message: "Signature for this request is not valid".to_string(),
        });
        assert!(err.is_fatal());
    }

    #[rstest]
    fn test_spot_http_rate_limit_is_retryable() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::BinanceError {
            code: -1015,
            message: "Too many new orders".to_string(),
        });
        assert!(err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_futures_http_rate_limit_is_retryable() {
        let err = BinanceError::FuturesHttp(BinanceFuturesHttpError::BinanceError {
            code: -1003,
            message: "Too many requests".to_string(),
        });
        assert!(err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_spot_http_unexpected_status_429_is_retryable() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::UnexpectedStatus {
            status: 429,
            body: "rate limited".to_string(),
        });
        assert!(err.is_retryable());
    }

    #[rstest]
    fn test_spot_http_unexpected_status_500_is_retryable() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::UnexpectedStatus {
            status: 500,
            body: "internal server error".to_string(),
        });
        assert!(err.is_retryable());
    }

    #[rstest]
    fn test_spot_http_unexpected_status_401_is_fatal() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::UnexpectedStatus {
            status: 401,
            body: "unauthorized".to_string(),
        });
        assert!(err.is_fatal());
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_display_formatting() {
        let err = BinanceError::SpotHttp(BinanceSpotHttpError::BinanceError {
            code: -1100,
            message: "Illegal characters found".to_string(),
        });
        let msg = err.to_string();
        assert!(msg.contains("Spot HTTP error"));
        assert!(msg.contains("-1100"));
    }
}
