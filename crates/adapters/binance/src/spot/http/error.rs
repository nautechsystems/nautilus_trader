//! Binance Spot HTTP error types.

use std::fmt::Display;

use nautilus_network::http::error::HttpClientError;

// Re-export unified SBE decode error
pub use crate::common::sbe::SbeDecodeError;

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
    },
    /// SBE decode error.
    SbeDecodeError(SbeDecodeError),
    /// JSON decode error.
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
        /// Response body (hex encoded for SBE).
        body: String,
    },
}

impl Display for BinanceSpotHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "Missing API credentials"),
            Self::BinanceError { code, message } => {
                write!(f, "Binance error {code}: {message}")
            }
            Self::SbeDecodeError(err) => write!(f, "SBE decode error: {err}"),
            Self::JsonError(msg) => write!(f, "JSON decode error: {msg}"),
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
            HttpClientError::Error(msg) => Self::NetworkError(msg),
        }
    }
}

/// Result type for Binance Spot HTTP operations.
pub type BinanceSpotHttpResult<T> = Result<T, BinanceSpotHttpError>;
