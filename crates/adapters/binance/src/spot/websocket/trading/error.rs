//! Binance Spot WebSocket API error types.

use std::fmt::Display;

use crate::common::sbe::error::SbeDecodeError;

/// Binance WebSocket API client error type.
#[derive(Debug)]
pub enum BinanceWsApiError {
    /// General client error.
    ClientError(String),
    /// Handler not available (channel closed).
    HandlerUnavailable(String),
    /// Connection error.
    ConnectionError(String),
    /// Authentication failed.
    AuthenticationError(String),
    /// Request rejected by venue.
    RequestRejected { code: i32, msg: String },
    /// SBE decoding error.
    DecodeError(SbeDecodeError),
    /// Request timed out.
    Timeout(String),
    /// Request ID not found in pending requests.
    UnknownRequestId(String),
}

impl Display for BinanceWsApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientError(msg) => write!(f, "Client error: {msg}"),
            Self::HandlerUnavailable(msg) => write!(f, "Handler unavailable: {msg}"),
            Self::ConnectionError(msg) => write!(f, "Connection error: {msg}"),
            Self::AuthenticationError(msg) => write!(f, "Authentication error: {msg}"),
            Self::RequestRejected { code, msg } => {
                write!(f, "Request rejected [{code}]: {msg}")
            }
            Self::DecodeError(err) => write!(f, "Decode error: {err}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::UnknownRequestId(id) => write!(f, "Unknown request ID: {id}"),
        }
    }
}

impl std::error::Error for BinanceWsApiError {}

impl From<SbeDecodeError> for BinanceWsApiError {
    fn from(err: SbeDecodeError) -> Self {
        Self::DecodeError(err)
    }
}

/// Result type for Binance WebSocket API operations.
pub type BinanceWsApiResult<T> = Result<T, BinanceWsApiError>;
