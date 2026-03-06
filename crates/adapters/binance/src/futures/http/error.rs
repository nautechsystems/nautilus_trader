//! Binance Futures HTTP error types.

use std::fmt::Display;

use nautilus_network::http::error::HttpClientError;

/// Binance Futures HTTP client error type.
#[derive(Debug)]
pub enum BinanceFuturesHttpError {
    /// Missing API credentials for authenticated request.
    MissingCredentials,
    /// Binance API returned an error response.
    BinanceError {
        /// Binance error code.
        code: i64,
        /// Error message from Binance.
        message: String,
    },
    /// JSON parsing or serialization error.
    JsonError(String),
    /// Request validation error.
    ValidationError(String),
    /// Network or connection error.
    NetworkError(String),
    /// Request timed out.
    Timeout(String),
    /// Request was canceled.
    Canceled(String),
    /// Unexpected HTTP status code.
    UnexpectedStatus {
        /// HTTP status code.
        status: u16,
        /// Response body.
        body: String,
    },
}

impl Display for BinanceFuturesHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "Missing API credentials"),
            Self::BinanceError { code, message } => {
                write!(f, "Binance error {code}: {message}")
            }
            Self::JsonError(msg) => write!(f, "JSON error: {msg}"),
            Self::ValidationError(msg) => write!(f, "Validation error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::Canceled(msg) => write!(f, "Canceled: {msg}"),
            Self::UnexpectedStatus { status, body } => {
                write!(f, "Unexpected status {status}: {body}")
            }
        }
    }
}

impl std::error::Error for BinanceFuturesHttpError {}

impl From<serde_json::Error> for BinanceFuturesHttpError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}

impl From<anyhow::Error> for BinanceFuturesHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<HttpClientError> for BinanceFuturesHttpError {
    fn from(err: HttpClientError) -> Self {
        match err {
            HttpClientError::TimeoutError(msg) => Self::Timeout(msg),
            HttpClientError::InvalidProxy(msg) | HttpClientError::ClientBuildError(msg) => {
                Self::NetworkError(msg)
            }
            HttpClientError::Error(msg) => Self::NetworkError(msg),
        }
    }
}

/// Result type for Binance Futures HTTP operations.
pub type BinanceFuturesHttpResult<T> = Result<T, BinanceFuturesHttpError>;
